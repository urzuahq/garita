//! Reports for the commands besides `run`: the same `Report`, with their own counts and next action.

use garita_core::report::{Examined, Finding, Report, RunContext, RunStatus, Severity};

use crate::engine::error_lines;
pub use crate::engine::finding::GARITA;

/// A command's report and the lines people read on stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub report: Report,
    pub summary: String,
}

impl Verdict {
    pub fn exit_code(&self) -> u8 {
        self.report.exit_code()
    }
}

/// The command to run next and why, when there is one.
pub type Next = Option<(String, String)>;

pub fn verdict(command: &str, findings: Vec<Finding>, examined: Examined, next: Next) -> Verdict {
    let mut report = Report::from_run(RunContext::default(), Vec::new(), findings, None);
    report.examined = examined;
    let (action, reason) = next.map_or((None, None), |(a, r)| (Some(a), Some(r)));
    report.next_action = action;
    report.next_action_reason = reason;
    Verdict {
        summary: summary(command, &report),
        report,
    }
}

pub fn not_run(command: &str, error: String) -> Verdict {
    let report = Report::not_run(RunContext::default(), error);
    Verdict {
        summary: summary(command, &report),
        report,
    }
}

pub fn finding(severity: Severity, rule: &str, message: String) -> Finding {
    crate::engine::finding::finding(GARITA, severity, rule, message)
}

fn summary(command: &str, report: &Report) -> String {
    let status = match report.status {
        RunStatus::Pass => "pass",
        RunStatus::Warn => "warn",
        RunStatus::Fail => "fail",
        RunStatus::NotRun => "not-run",
    };
    let examined: Vec<String> = report
        .examined
        .iter()
        .map(|(unit, count)| format!("{count} {unit}"))
        .collect();
    let examined = if examined.is_empty() {
        String::new()
    } else {
        format!("examined {}; ", examined.join(", "))
    };
    let mut lines = vec![format!(
        "garita {command}: {status} ({examined}{} findings)",
        report.findings.len()
    )];
    if let Some(error) = &report.error {
        lines.extend(error_lines(error));
    }
    for f in &report.findings {
        let severity = match f.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        lines.push(format!("  {severity} {}: {}", f.rule, f.message));
    }
    if let Some(next) = &report.next_action {
        lines.push(format!("  next: {next}"));
    }
    lines.join("\n")
}
