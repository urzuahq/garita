use garita_core::report::{CheckStatus, Report, RunStatus, Severity};

/// The error under one `error:` label, its later lines indented beneath it.
pub fn error_lines(error: &str) -> Vec<String> {
    let mut text = error.lines().filter(|l| !l.trim().is_empty());
    let first = text.next().map(|l| format!("  error: {l}"));
    first
        .into_iter()
        .chain(text.map(|l| format!("    {}", l.trim())))
        .collect()
}

/// A few lines for people: the status, each check that did not pass with its most severe
/// finding, and the next action.
pub fn summary(report: &Report) -> String {
    let status = match report.status {
        RunStatus::Pass => "pass",
        RunStatus::Warn => "warn",
        RunStatus::Fail => "fail",
        RunStatus::NotRun => "not-run",
    };
    let at = report
        .trigger
        .map_or_else(String::new, |t| format!(" at {}", t.as_str()));
    let mut lines = vec![format!(
        "garita: {status}{at} ({} checks, {} findings)",
        report.checks.len(),
        report.findings.len()
    )];
    if let Some(error) = &report.error {
        lines.extend(error_lines(error));
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
        let detail = headline(report, &check.name);
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

fn rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

/// ` - <place><rule> <message>` for the check's first finding of its worst severity, with how
/// many more share that severity.
/// A message's first line, joined with the first line of output after it when the first line only
/// introduces that output (`exited with code 1:`), so the reason shows in the summary.
fn brief(message: &str) -> String {
    let mut lines = message.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next().unwrap_or_default();
    if !first.ends_with(':') {
        return first.to_string();
    }
    match lines.find(|l| !matches!(*l, "stderr:" | "stdout:")) {
        Some(reason) => format!("{first} {reason}"),
        None => first.to_string(),
    }
}

fn headline(report: &Report, check: &str) -> String {
    let own: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.check == check)
        .collect();
    let Some(worst) = own.iter().map(|f| rank(f.severity)).min() else {
        return String::new();
    };
    let mut same = own.iter().filter(|f| rank(f.severity) == worst);
    let Some(f) = same.next() else {
        return String::new();
    };
    let place = match (&f.file, f.line) {
        (Some(file), Some(line)) => format!("{file}:{line}: "),
        (Some(file), None) => format!("{file}: "),
        _ => String::new(),
    };
    let message = brief(&f.message);
    let more = match same.count() {
        0 => String::new(),
        n => {
            let noun = match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "info finding",
            };
            let s = if n == 1 { "" } else { "s" };
            format!(" (+{n} more {noun}{s})")
        }
    };
    format!(" - {place}{} {message}{more}", f.rule)
}

#[cfg(test)]
mod tests {
    use garita_core::report::{CheckEntry, CheckKind, Finding, RunContext};

    use super::*;

    fn finding(severity: Severity, rule: &str, file: Option<&str>) -> Finding {
        Finding {
            check: "records".into(),
            severity,
            rule: rule.into(),
            file: file.map(Into::into),
            line: None,
            column: None,
            message: format!("{rule} said so\nmore"),
            fix: None,
            fingerprint: None,
        }
    }

    fn summary_of(findings: Vec<Finding>) -> String {
        let check = CheckEntry {
            name: "records".into(),
            kind: CheckKind::Other,
            status: CheckStatus::Fail,
            duration_ms: 1,
            examined: Default::default(),
            rerun: None,
        };
        summary(&Report::from_run(
            RunContext::default(),
            vec![check],
            findings,
            None,
        ))
    }

    #[test]
    fn a_failing_check_headlines_its_first_error_not_its_first_finding() {
        let text = summary_of(vec![
            finding(Severity::Info, "outside-root", None),
            finding(Severity::Warning, "w", None),
            finding(Severity::Error, "exit-code/failed", Some("a.ts")),
            finding(Severity::Error, "second", None),
            finding(Severity::Error, "third", None),
        ]);
        let line = text.lines().nth(1).unwrap_or_default();
        assert_eq!(
            line,
            "  records: fail - a.ts: exit-code/failed exit-code/failed said so (+2 more errors)"
        );
    }

    #[test]
    fn a_message_that_introduces_output_shows_the_first_line_of_it() {
        assert_eq!(
            brief("exited with code 1:\nstderr:\n--fail-on-drift set: failing\nstdout:\n{}"),
            "exited with code 1: --fail-on-drift set: failing"
        );
        assert_eq!(
            brief("exited with code 3:\nboom\nmore"),
            "exited with code 3: boom"
        );
        assert_eq!(
            brief("Unexpected `debugger`\nhelp"),
            "Unexpected `debugger`"
        );
        assert_eq!(brief("exited with code 1:"), "exited with code 1:");
    }

    #[test]
    fn a_lone_finding_of_its_severity_has_no_count() {
        let text = summary_of(vec![
            finding(Severity::Info, "i", None),
            finding(Severity::Error, "e", None),
            finding(Severity::Warning, "w1", None),
            finding(Severity::Warning, "w2", None),
        ]);
        assert!(text.contains("  records: fail - e e said so\n"), "{text}");
    }
}
