use std::time::Duration;

use garita_core::adapter::ToolFinding;
use garita_core::report::{Finding, Severity};

pub fn finding(check: &str, severity: Severity, rule: &str, message: String) -> Finding {
    tool_finding(severity, rule, None, message).into_finding(check)
}

/// A finding about the run itself, which carries no check.
pub fn run_finding(severity: Severity, rule: &str, message: String) -> Finding {
    tool_finding(severity, rule, None, message).into_run_finding()
}

pub fn run_file_finding(
    severity: Severity,
    rule: &str,
    file: Option<String>,
    message: String,
) -> Finding {
    tool_finding(severity, rule, file, message).into_run_finding()
}

fn tool_finding(
    severity: Severity,
    rule: &str,
    file: Option<String>,
    message: String,
) -> ToolFinding {
    ToolFinding {
        severity,
        rule: rule.to_string(),
        file,
        line: None,
        column: None,
        message,
        fix: None,
    }
}

/// `1.234s`, or `412ms` below a second.
pub fn seconds(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{}.{:03}s", ms / 1000, ms % 1000)
    }
}

pub fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
