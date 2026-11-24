use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use garita_core::adapter::{
    Adapter, Outcome as Parsed, ParseContext, ToolOutput, fingerprint, last_lines, notable,
    printed_nothing,
};
use garita_core::config::{Check, Config, OnCrash, OnExceed};
use garita_core::report::{
    CheckEntry, CheckKind, CheckStatus, Examined, Finding, RunError, Severity, Trigger,
    rerun_command,
};

use super::exec::{Executed, UnitRun, command_line};
use super::finding::{finding, millis, plural, seconds};
use super::plan::Plan;
use super::turbo::TaskLog;
use crate::process::{Finished, Outcome};

/// What the receipt records about a check beyond its report entry.
#[derive(Debug, Clone)]
pub struct Record {
    pub command: String,
    pub files: Vec<String>,
    pub started: SystemTime,
    pub finished: SystemTime,
    pub exit_code: Option<i32>,
    pub output: Option<PathBuf>,
    pub tail: String,
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub entry: CheckEntry,
    pub findings: Vec<Finding>,
    /// Why the check could not run, when it could not.
    pub not_run: Option<RunError>,
    pub record: Record,
}

impl CheckResult {
    /// A check that did not run its tool: skipped, or not runnable.
    pub fn without_run(
        trigger: Trigger,
        name: &str,
        kind: CheckKind,
        status: CheckStatus,
        findings: Vec<Finding>,
        not_run: Option<RunError>,
        record: Record,
    ) -> Self {
        Self {
            entry: CheckEntry {
                rerun: Some(rerun_command(Some(trigger), name, &[])),
                ..CheckEntry::new(name, kind, status)
            },
            findings,
            not_run,
            record,
        }
    }
}

pub fn kind_of(check: &Check, adapter: Option<Adapter>) -> CheckKind {
    check
        .kind
        .or(adapter.map(Adapter::kind))
        .unwrap_or(CheckKind::Other)
}

struct Tally<'a> {
    config: &'a Config,
    plan: &'a Plan<'a>,
    findings: Vec<Finding>,
    examined: Examined,
    not_run: Option<RunError>,
    suppressed: BTreeMap<Severity, u64>,
    notable: Vec<String>,
}

pub fn judge(config: &Config, plan: &Plan<'_>, executed: Executed) -> CheckResult {
    let mut tally = Tally {
        config,
        plan,
        findings: plan.findings.clone(),
        examined: Examined::new(),
        not_run: None,
        suppressed: BTreeMap::new(),
        notable: Vec::new(),
    };
    for run in &executed.runs {
        match &run.outcome {
            Err(error) => {
                let message = format!("checks.{}: {}", plan.check.name, error.message);
                tally.not_run = Some(RunError::new(error.kind, message));
            }
            Ok(Outcome::CouldNotStart { reason }) => {
                let message = format!("checks.{}: {reason}", plan.check.name);
                tally.not_run = Some(RunError::environment(message));
            }
            Ok(Outcome::Ran(finished)) => tally.unit(run, finished),
        }
    }
    tally.budget(executed.duration);
    let check = plan.check;
    if check.advisory {
        for f in &mut tally.findings {
            if f.severity == Severity::Error {
                f.severity = Severity::Warning;
            }
        }
    }
    let status = match &tally.not_run {
        Some(_) => CheckStatus::NotRun,
        None => status_of(&tally.findings),
    };
    let entry = CheckEntry {
        name: check.name.clone(),
        kind: kind_of(check, Some(plan.adapter)),
        status,
        duration_ms: millis(executed.duration),
        examined: tally.examined,
        rerun: Some(rerun_command(
            Some(plan.trigger),
            &check.name,
            &if plan.given.is_some() {
                error_files(&tally.findings)
            } else {
                Vec::new()
            },
        )),
        output_tail: None,
        output: None,
        output_notable: tally.notable,
        suppressed: tally.suppressed,
    };
    let units: Vec<_> = executed.runs.iter().map(|r| r.unit.clone()).collect();
    let record = Record {
        command: command_line(&units),
        files: plan.files.clone(),
        started: executed.started,
        finished: executed.finished,
        exit_code: exit_code(&executed.runs),
        output: executed.output,
        tail: executed.tail,
    };
    CheckResult {
        entry,
        findings: tally.findings,
        not_run: tally.not_run,
        record,
    }
}

impl Tally<'_> {
    fn name(&self) -> &str {
        &self.plan.check.name
    }

    fn unit(&mut self, run: &UnitRun, finished: &Finished) {
        if finished.timed_out {
            let why = format!(
                "timed out after {} and was killed",
                seconds(self.plan.timeout)
            );
            return self.crash(&why);
        }
        if let Some(signal) = finished.signal {
            return self.crash(&format!("was killed by signal {signal}"));
        }
        match &run.turbo {
            Some(tasks) => self.turbo(tasks, finished),
            None => {
                let output = ToolOutput {
                    stdout: &finished.stdout,
                    stderr: &finished.stderr,
                    exit_code: finished.exit_code,
                };
                let given = self.plan.given.map(|_| run.unit.files);
                self.parsed(&output, &run.unit.cwd, given);
            }
        }
    }

    fn parsed(&mut self, output: &ToolOutput<'_>, cwd: &str, given: Option<u64>) {
        let context = ParseContext { cwd, files: given };
        let parsed = self.plan.adapter.parse(output, &context);
        for (unit, count) in parsed.examined {
            let sum = self.examined.entry(unit).or_insert(0);
            *sum = sum.saturating_add(count);
        }
        self.note(parsed.notable);
        let name = self.name().to_string();
        let floor = self.plan.check.min_severity;
        for f in parsed.findings {
            if f.severity > floor {
                *self.suppressed.entry(f.severity).or_insert(0) += 1;
            } else {
                self.findings.push(f.into_finding(&name));
            }
        }
        match parsed.outcome {
            Parsed::Clean | Parsed::Findings => {}
            Parsed::Crashed { reason } => self.crash(&reason),
            Parsed::Nothing => self.nothing(output, cwd, given),
        }
    }

    /// A tool that examined nothing is a finding, never a pass: an error when it also exited
    /// non-zero, else a warning.
    fn nothing(&mut self, output: &ToolOutput<'_>, cwd: &str, given: Option<u64>) {
        let name = self.name().to_string();
        let place = if cwd.is_empty() {
            String::new()
        } else {
            format!(" in {cwd}")
        };
        let code = output.exit_code.filter(|c| *c != 0);
        let message = match (code, given.filter(|n| *n > 0)) {
            (Some(code), _) => {
                self.note(notable(output.stderr, output.stdout));
                format!("{name} exited with code {code} and examined nothing{place}")
            }
            (None, Some(n)) => format!(
                "{name} was given {}{place} and examined none",
                plural(usize::try_from(n).unwrap_or(usize::MAX), "file")
            ),
            (None, None) => format!("{name} examined nothing{place}"),
        };
        let severity = if code.is_some() {
            Severity::Error
        } else {
            Severity::Warning
        };
        self.findings
            .push(finding(&name, severity, "no-op", message));
    }

    fn turbo(&mut self, tasks: &Result<Vec<TaskLog>, String>, finished: &Finished) {
        let errors_before = self.errors();
        match tasks {
            Ok(tasks) => {
                for task in tasks {
                    match &task.log {
                        Some(log) => self.task_log(task, log),
                        None => self.task_without_log(task, finished),
                    }
                }
            }
            Err(reason) => {
                if printed_nothing(&finished.stderr, &finished.stdout) {
                    self.unrecognized(format!("{reason}; turbo printed nothing"));
                } else {
                    self.note(notable(&finished.stderr, &finished.stdout));
                    self.unrecognized(reason.clone());
                }
            }
        }
        let code = finished.exit_code.unwrap_or_default();
        if code != 0 && self.errors() == errors_before {
            self.note(notable(&finished.stderr, &finished.stdout));
            self.unrecognized(format!(
                "turbo exited with code {code} but no task reported an error"
            ));
        }
    }

    fn task_log(&mut self, task: &TaskLog, log: &str) {
        let output = ToolOutput {
            stdout: log,
            stderr: "",
            exit_code: task.exit_code,
        };
        let start = self.findings.len();
        let noted = self.notable.len();
        self.parsed(&output, &task.directory, None);
        let unplaced = self
            .findings
            .iter()
            .skip(start)
            .any(|f| f.severity == Severity::Error && f.file.is_none());
        // A task's log is not the check's output, so without a picked-out line its last lines
        // stand in for the tail the finding cannot carry.
        if unplaced && self.notable.len() == noted {
            self.note(last_lines(log));
        }
        for line in self.notable.iter_mut().skip(noted) {
            *line = format!("{}: {line}", task.task);
        }
        for f in self.findings.iter_mut().skip(start) {
            if f.file.is_none() {
                f.message = format!("{}: {}", task.task, f.message);
                f.fingerprint = Some(fingerprint(&f.rule, None, &f.message));
            }
        }
    }

    /// A task with no log file wrote its output only to turbo's own, so that is the detail.
    fn task_without_log(&mut self, task: &TaskLog, finished: &Finished) {
        let failed = match task.exit_code {
            Some(code) => code != 0,
            None => finished.exit_code != Some(0),
        };
        if !failed {
            if task.exit_code.is_none() {
                let name = self.name().to_string();
                let message = format!("{} has no execution record", task.task);
                self.findings
                    .push(finding(&name, Severity::Warning, "no-op", message));
            }
            return;
        }
        let code = task.exit_code.map_or_else(
            || "did not finish".to_string(),
            |c| format!("exited with code {c}"),
        );
        self.note(notable(&finished.stderr, &finished.stdout));
        let message = format!("{}: {code}", task.task);
        let name = self.name().to_string();
        self.findings.push(finding(
            &name,
            Severity::Error,
            "turbo/task-failed",
            message,
        ));
    }

    /// Adds lines to the check's notable ones, each once.
    fn note(&mut self, lines: Vec<String>) {
        for line in lines {
            if !self.notable.contains(&line) {
                self.notable.push(line);
            }
        }
    }

    fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }

    fn unrecognized(&mut self, message: String) {
        let name = self.name().to_string();
        self.findings.push(finding(
            &name,
            Severity::Error,
            "turbo/unrecognized-output",
            message,
        ));
    }

    fn crash(&mut self, why: &str) {
        let severity = match self.config.on_crash {
            OnCrash::Fail => Severity::Error,
            OnCrash::Warn => Severity::Warning,
        };
        let name = self.name().to_string();
        let message = format!("{name} {why}; it gave no verdict of its own");
        self.findings
            .push(finding(&name, severity, "crash", message));
    }

    fn budget(&mut self, took: Duration) {
        let check = self.plan.check;
        let Some(budget) = check.budget.map(|b| b.duration) else {
            return;
        };
        if took <= budget {
            return;
        }
        let severity = match self.config.on_exceed(check) {
            OnExceed::Fail => Severity::Error,
            OnExceed::Warn | OnExceed::Demote => Severity::Warning,
        };
        let message = format!(
            "{} took {}, over its budget of {}",
            check.name,
            seconds(took),
            seconds(budget)
        );
        let name = check.name.clone();
        self.findings
            .push(finding(&name, severity, "budget", message));
    }
}

pub fn status_of(findings: &[Finding]) -> CheckStatus {
    if findings.iter().any(|f| f.severity == Severity::Error) {
        CheckStatus::Fail
    } else if findings.iter().any(|f| f.severity == Severity::Warning) {
        CheckStatus::Warn
    } else {
        CheckStatus::Pass
    }
}

fn error_files(findings: &[Finding]) -> Vec<&str> {
    let mut files: Vec<&str> = Vec::new();
    for f in findings.iter().filter(|f| f.severity == Severity::Error) {
        if let Some(file) = f.file.as_deref().filter(|f| !files.contains(f)) {
            files.push(file);
        }
    }
    files
}

/// The first non-zero exit among the units, else zero; `None` when one was killed or never ran.
fn exit_code(runs: &[UnitRun]) -> Option<i32> {
    let mut codes = Vec::new();
    for run in runs {
        match &run.outcome {
            Ok(Outcome::Ran(f)) => codes.push(f.exit_code?),
            _ => return None,
        }
    }
    codes.iter().copied().find(|c| *c != 0).or(Some(0))
}
