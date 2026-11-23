//! Reports for the commands besides `run`: the same `Report`, with their own counts and next action.

use garita_core::report::{Examined, Finding, Report, RunContext, Severity};

/// A command's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub report: Report,
}

impl Verdict {
    pub fn exit_code(&self) -> u8 {
        self.report.exit_code()
    }
}

/// The command to run next and why, when there is one.
pub type Next = Option<(String, String)>;

pub fn verdict(findings: Vec<Finding>, examined: Examined, next: Next) -> Verdict {
    let mut report = Report::from_run(RunContext::default(), Vec::new(), findings, None);
    report.examined = examined;
    let (action, reason) = next.map_or((None, None), |(a, r)| (Some(a), Some(r)));
    report.next_action = action;
    report.next_action_reason = reason;
    Verdict { report }
}

pub fn not_run(error: String) -> Verdict {
    Verdict {
        report: Report::not_run(RunContext::default(), error),
    }
}

pub fn finding(severity: Severity, rule: &str, message: String) -> Finding {
    crate::engine::finding::run_finding(severity, rule, message)
}
