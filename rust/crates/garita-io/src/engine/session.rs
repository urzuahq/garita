use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use garita_core::ObjectId;
use garita_core::config::{Check, CheckForm, Config, OnExceed, satisfies};
use garita_core::receipt::file_name;
use garita_core::report::{
    CheckKind, CheckStatus, Finding, Report, RunContext, Severity, Trigger, rerun_command,
};

use super::exec;
use super::finding::{GARITA, finding, seconds};
use super::judge::{self, CheckResult, Record};
use super::plan::{self, Planned};
use super::record::{self, RunFacts};
use super::scope::{self, Resolved};
use super::select::{self, Pick};
use super::{Clock, Outcome, Request};
use crate::config_file::{self, Loaded};
use crate::git;
use crate::receipts::{self, Listing};

pub fn run(request: &Request, clock: &dyn Clock) -> Outcome {
    let trigger = request.trigger;
    let root = match git::root(&request.dir) {
        Ok(root) => root,
        Err(e) => return Outcome::not_run(trigger, None, e.to_string()),
    };
    let tree = tree_of(&root, trigger).ok();
    let loaded = match config_file::load(&root) {
        Ok(loaded) => loaded,
        Err(e) => {
            let tree = tree.map(|t| t.as_str().to_string());
            return Outcome::not_run(trigger, tree, e.to_string());
        }
    };
    let shown = tree.as_ref().map(|t| t.as_str().to_string());
    match Session::start(root, request, clock, loaded, tree) {
        Ok(session) => session.run(),
        Err(e) => Outcome::not_run(trigger, shown, e),
    }
}

/// The index's tree at the commit trigger, where no commit exists yet; else HEAD's tree.
fn tree_of(root: &Path, trigger: Trigger) -> Result<ObjectId, String> {
    let head = git::head_commit(root).map_err(|e| e.to_string())?;
    match (trigger, head) {
        (Trigger::Commit, _) | (_, None) => git::tree_of_index(root),
        (_, Some(commit)) => git::tree_of_commit(root, commit.as_str()),
    }
    .map_err(|e| e.to_string())
}

struct Session<'a> {
    root: PathBuf,
    request: &'a Request,
    clock: &'a dyn Clock,
    loaded: Loaded,
    run_id: String,
    started: SystemTime,
    timer: Instant,
    commit: Option<ObjectId>,
    tree: ObjectId,
    worktree: ObjectId,
    dirty: bool,
    notes: Vec<String>,
}

impl<'a> Session<'a> {
    fn start(
        root: PathBuf,
        request: &'a Request,
        clock: &'a dyn Clock,
        loaded: Loaded,
        tree: Option<ObjectId>,
    ) -> Result<Self, String> {
        let started = clock.now();
        let timer = Instant::now();
        let commit = match request.trigger {
            Trigger::Commit => None,
            _ => git::head_commit(&root).map_err(|e| e.to_string())?,
        };
        let tree = match tree {
            Some(tree) => tree,
            None => tree_of(&root, request.trigger)?,
        };
        let worktree = git::worktree_tree(&root).map_err(|e| e.to_string())?;
        Ok(Self {
            dirty: worktree != tree,
            root,
            request,
            clock,
            loaded,
            run_id: crate::clock::run_id(),
            started,
            timer,
            commit,
            tree,
            worktree,
            notes: Vec::new(),
        })
    }

    fn run(mut self) -> Outcome {
        if let Some(error) = requires_error(&self.loaded.config, &self.request.version) {
            return self.finish(None, Vec::new(), Some(error));
        }
        let mut scope = match scope::resolve(&self.root, &self.loaded.config, self.request) {
            Ok(scope) => scope,
            Err(e) => {
                let error = format!("could not compute the scope: {e}");
                return self.finish(None, Vec::new(), Some(error));
            }
        };
        let history = receipts::list(&self.root).unwrap_or_else(|e| {
            self.notes.push(format!(
                "garita: could not read past receipts, so nothing is demoted: {e}"
            ));
            Listing::default()
        });
        let config = &self.loaded.config;
        let picks = select::select(config, self.request.trigger, &self.request.checks, &history);
        if picks.is_empty() && self.request.checks.is_empty() {
            let message = format!(
                "no check is configured for the `{}` trigger",
                self.request.trigger.as_str()
            );
            scope
                .findings
                .push(finding(GARITA, Severity::Warning, "no-checks", message));
        }
        let results = self.results(&picks, &scope);
        self.finish(Some(&scope), results, None)
    }

    fn results(&self, picks: &[Pick<'_>], scope: &Resolved) -> Vec<CheckResult> {
        let config = &self.loaded.config;
        let mut slots: Vec<Option<CheckResult>> = Vec::new();
        let mut plans = Vec::new();
        let mut plan_slots = Vec::new();
        for pick in picks {
            let result = match pick {
                Pick::Run(check) => {
                    match plan::plan(&self.root, config, self.request.trigger, check, scope) {
                        Planned::Run(plan) => {
                            plan_slots.push(slots.len());
                            plans.push(plan);
                            None
                        }
                        Planned::Skip {
                            check,
                            files,
                            findings,
                        } => Some(self.unrun(
                            &check.name,
                            Some(check),
                            CheckStatus::Skipped,
                            findings,
                            None,
                            files,
                        )),
                        Planned::NotRun { check, reason } => Some(self.unrun(
                            &check.name,
                            Some(check),
                            CheckStatus::NotRun,
                            Vec::new(),
                            Some(reason),
                            Vec::new(),
                        )),
                    }
                }
                Pick::Skip(check, f) => Some(self.unrun(
                    &check.name,
                    Some(check),
                    CheckStatus::Skipped,
                    vec![f.clone()],
                    None,
                    Vec::new(),
                )),
                Pick::NotRun {
                    name,
                    check,
                    reason,
                } => Some(self.unrun(
                    name,
                    *check,
                    CheckStatus::NotRun,
                    Vec::new(),
                    Some(reason.clone()),
                    Vec::new(),
                )),
            };
            slots.push(result);
        }
        let executed = exec::run_all(&self.root, &self.run_id, &plans);
        for ((plan, done), slot) in plans.iter().zip(executed).zip(plan_slots) {
            if let Some(entry) = slots.get_mut(slot) {
                *entry = Some(judge::judge(config, plan, done));
            }
        }
        slots.into_iter().flatten().collect()
    }

    /// A check whose tool never ran; its output file says why.
    fn unrun(
        &self,
        name: &str,
        check: Option<&Check>,
        status: CheckStatus,
        findings: Vec<Finding>,
        not_run: Option<String>,
        files: Vec<String>,
    ) -> CheckResult {
        let now = SystemTime::now();
        let why: Vec<&str> = not_run
            .iter()
            .map(String::as_str)
            .chain(findings.iter().map(|f| f.message.as_str()))
            .collect();
        let output = receipts::output_path(&self.root, &self.run_id, name)
            .ok()
            .filter(|path| fs::write(path, format!("garita: {}\n", why.join("\n"))).is_ok());
        let tail = output.as_deref().map(exec::read_tail).unwrap_or_default();
        let record = Record {
            command: check.map(declared_command).unwrap_or_default(),
            files,
            started: now,
            finished: now,
            exit_code: None,
            output,
            tail,
        };
        let kind = check.map_or(CheckKind::Other, |c| judge::kind_of(c, adapter_of(c)));
        CheckResult::without_run(
            self.request.trigger,
            name,
            kind,
            status,
            findings,
            not_run,
            record,
        )
    }

    fn finish(
        mut self,
        scope: Option<&Resolved>,
        results: Vec<CheckResult>,
        error: Option<String>,
    ) -> Outcome {
        let mut findings: Vec<Finding> = scope.map(|s| s.findings.clone()).unwrap_or_default();
        findings.extend(results.iter().flat_map(|r| r.findings.iter().cloned()));
        findings.extend(self.trigger_budget());
        let reasons: Vec<&str> = results
            .iter()
            .filter_map(|r| r.not_run.as_deref())
            .collect();
        let error = error.or_else(|| (!reasons.is_empty()).then(|| reasons.join("\n")));
        let trigger = self.request.trigger;
        let started_at = crate::clock::rfc3339(self.started);
        let receipt_path = file_name(&started_at, trigger, &self.run_id)
            .map(|name| format!("{}/{name}", receipts::DIR))
            .ok();
        let context = RunContext {
            trigger: Some(trigger),
            commit: self.commit.as_ref().map(|c| c.as_str().to_string()),
            tree: Some(self.tree.as_str().to_string()),
            scope: scope.map(Resolved::report_scope),
            receipt: receipt_path,
        };
        let entries = results.iter().map(|r| r.entry.clone()).collect();
        let mut report = Report::from_run(context, entries, findings, error);
        if report.next_action.as_deref() == Some(rerun_command(Some(trigger), GARITA, &[]).as_str())
        {
            report.next_action = Some(format!("garita run --trigger {}", trigger.as_str()));
        }
        let finished_at = crate::clock::rfc3339(self.clock.now());
        let facts = RunFacts {
            root: &self.root,
            run_id: &self.run_id,
            started_at: &started_at,
            finished_at: &finished_at,
            trigger,
            commit: self.commit.clone(),
            tree: self.tree.clone(),
            worktree: self.worktree.clone(),
            dirty: self.dirty,
            config_hash: self.loaded.hash.as_str(),
            version: &self.request.version,
            scope: scope.map_or(self.loaded.config.trigger(trigger).scope, |s| s.kind),
        };
        let receipt = record::receipt(&facts, &results, &report);
        match receipts::write(&self.root, &receipt) {
            Ok(_) => self.prune(),
            Err(e) => {
                self.notes
                    .push(format!("garita: could not write the receipt: {e}"));
                report.receipt = None;
            }
        }
        Outcome {
            report,
            notes: self.notes,
        }
    }

    fn prune(&mut self) {
        let keep = usize::try_from(self.loaded.config.receipts_keep).unwrap_or(usize::MAX);
        if let Err(e) = receipts::prune(&self.root, keep) {
            self.notes
                .push(format!("garita: could not prune old receipts: {e}"));
        }
    }

    fn trigger_budget(&self) -> Option<Finding> {
        let trigger = self.request.trigger;
        let budget = self.loaded.config.trigger(trigger).budget?.duration;
        let took = self.timer.elapsed();
        (took > budget).then(|| {
            let severity = match self.loaded.config.budgets.on_exceed {
                OnExceed::Fail => Severity::Error,
                OnExceed::Warn | OnExceed::Demote => Severity::Warning,
            };
            let message = format!(
                "the {} trigger took {}, over its budget of {}",
                trigger.as_str(),
                seconds(took),
                seconds(budget)
            );
            finding(GARITA, severity, "budget", message)
        })
    }
}

/// Why the config's `requires` rules out this garita, when it does.
pub(crate) fn requires_error(config: &Config, version: &str) -> Option<String> {
    let requires = config.requires.as_ref()?;
    let path = config_file::PATH;
    match satisfies(requires, version) {
        Ok(true) => None,
        Ok(false) => Some(format!(
            "{path}: requires: this config needs garita {}, and this is garita {version}",
            requires.text
        )),
        Err(e) => Some(format!(
            "{path}: requires: garita's own version `{version}` cannot be compared: {e}"
        )),
    }
}

fn declared_command(check: &Check) -> String {
    match &check.form {
        CheckForm::Run { run, .. } => run.clone(),
        CheckForm::Native { tool } => format!("native: {tool}"),
        CheckForm::Builtin { builtin } => format!("builtin: {}", builtin.as_str()),
    }
}

fn adapter_of(check: &Check) -> Option<garita_core::adapter::Adapter> {
    match &check.form {
        CheckForm::Run { parser, .. } => {
            garita_core::adapter::Adapter::from_parser_name(parser).ok()
        }
        _ => None,
    }
}
