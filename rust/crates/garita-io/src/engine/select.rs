use std::time::Duration;

use garita_core::config::{Check, CheckForm, Config, OnExceed};
use garita_core::report::{CheckStatus, Finding, RunError, Severity, Trigger};

use super::finding::{finding, seconds};
use crate::receipts::Listing;

/// What the run does with one check.
#[derive(Debug)]
pub enum Pick<'a> {
    Run(&'a Check),
    /// Skipped before it ran, with the finding that says why.
    Skip(&'a Check, Finding),
    /// A check in the config whose form this version does not support.
    NotRun {
        check: &'a Check,
        reason: RunError,
    },
    /// A name on the command line that is not a check; it gets no entry in the report.
    Unknown(String),
}

/// The checks named on the command line, else those whose `on` has the trigger, less the demoted.
pub fn select<'a>(
    config: &'a Config,
    trigger: Trigger,
    named: &[String],
    history: &Listing,
) -> Vec<Pick<'a>> {
    if !named.is_empty() {
        let mut seen: Vec<&str> = Vec::new();
        return named
            .iter()
            .filter(|n| {
                let first = !seen.contains(&n.as_str());
                seen.push(n.as_str());
                first
            })
            .map(|name| match config.check(name) {
                Some(check) => supported(check),
                None => Pick::Unknown(name.clone()),
            })
            .collect();
    }
    config
        .checks
        .iter()
        .filter(|c| c.on.contains(&trigger))
        .map(|check| match demoted(config, check, trigger, history) {
            Some(finding) => Pick::Skip(check, finding),
            None => supported(check),
        })
        .collect()
}

fn supported(check: &Check) -> Pick<'_> {
    let form = match &check.form {
        CheckForm::Run { .. } => return Pick::Run(check),
        CheckForm::Native { tool } => format!("`native: {tool}`"),
        CheckForm::Builtin { builtin } => format!("`builtin: {}`", builtin.as_str()),
    };
    Pick::NotRun {
        check,
        reason: RunError::config(format!(
            "checks.{}: the {form} form is not supported by this version of garita",
            check.name
        )),
    }
}

/// The `demoted` finding when the last run of `check` at `trigger` went over its budget, exited
/// on its own and passed or warned; a check that failed, was killed or could not run keeps
/// gating here, so a tool that hangs is never skipped for hanging.
fn demoted(config: &Config, check: &Check, trigger: Trigger, history: &Listing) -> Option<Finding> {
    if config.on_exceed(check) != OnExceed::Demote {
        return None;
    }
    let later = check.next_trigger(trigger)?;
    let budget = check.budget?.duration;
    let last = history.last_run_of(&check.name, trigger)?;
    let entry = last.receipt.checks.iter().find(|c| c.name == check.name)?;
    let took = Duration::from_millis(entry.duration_ms);
    let gated =
        matches!(entry.status, CheckStatus::Pass | CheckStatus::Warn) && entry.exit_code.is_some();
    (gated && took > budget).then(|| {
        let message = format!(
            "demoted to {}: its last run at {} took {}, over its budget of {}; \
             `garita run {} --trigger {}` runs it here again",
            later.as_str(),
            trigger.as_str(),
            seconds(took),
            seconds(budget),
            check.name,
            trigger.as_str(),
        );
        finding(&check.name, Severity::Info, "demoted", message)
    })
}
