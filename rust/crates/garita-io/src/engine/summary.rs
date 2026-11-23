use garita_core::report::{CheckStatus, Report, RunStatus, Severity};

/// A few lines for people: the status, each check that did not pass with its first finding,
/// and the next action.
pub fn summary(report: &Report) -> String {
    let status = match report.status {
        RunStatus::Pass => "pass",
        RunStatus::Warn => "warn",
        RunStatus::Fail => "fail",
        RunStatus::NotRun => "not-run",
    };
    let trigger = report.trigger.map_or("", |t| t.as_str());
    let mut lines = vec![format!(
        "garita: {status} at {trigger} ({} checks, {} findings)",
        report.checks.len(),
        report.findings.len()
    )];
    if let Some(error) = &report.error {
        lines.extend(error.lines().map(|l| format!("  error: {l}")));
    }
    for check in report
        .checks
        .iter()
        .filter(|c| c.status != CheckStatus::Pass)
    {
        let status = match check.status {
            CheckStatus::Pass => "pass",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "fail",
            CheckStatus::Skipped => "skipped",
            CheckStatus::NotRun => "not-run",
        };
        let first = report.findings.iter().find(|f| f.check == check.name);
        let detail = first.map_or_else(String::new, |f| {
            let place = match (&f.file, f.line) {
                (Some(file), Some(line)) => format!("{file}:{line}: "),
                (Some(file), None) => format!("{file}: "),
                _ => String::new(),
            };
            let message = f.message.lines().next().unwrap_or_default();
            format!(" - {place}{} {message}", f.rule)
        });
        lines.push(format!("  {}: {status}{detail}", check.name));
    }
    let checks: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    for f in report
        .findings
        .iter()
        .filter(|f| !checks.contains(&f.check.as_str()) && f.severity != Severity::Info)
    {
        lines.push(format!("  {}: {} {}", f.check, f.rule, f.message));
    }
    if let Some(next) = &report.next_action {
        lines.push(format!("  next: {next}"));
    }
    lines.join("\n")
}
