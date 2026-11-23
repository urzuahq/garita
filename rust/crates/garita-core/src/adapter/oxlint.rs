use serde::Deserialize;

use super::{ParseContext, Parsed, ToolFinding, ToolOutput, nonzero, rebase};
use crate::argv::quote;
use crate::report::Severity;

const NO_FILES: &str = "No files found to lint";

#[derive(Deserialize)]
struct Report {
    diagnostics: Vec<Diagnostic>,
    number_of_files: u64,
}

#[derive(Deserialize)]
struct Diagnostic {
    message: String,
    #[serde(default)]
    code: Option<String>,
    severity: String,
    #[serde(default)]
    help: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct Label {
    span: Span,
}

#[derive(Deserialize)]
struct Span {
    line: u64,
    column: u64,
}

/// Parses `oxlint --format json`, which may be preceded by a line of prose.
pub fn parse(output: &ToolOutput<'_>, context: &ParseContext<'_>) -> Parsed {
    let Some(exit) = output.exit_code else {
        return Parsed::killed();
    };
    let Some((prose, json)) = split_json(output.stdout) else {
        return Parsed::crashed(no_report_reason(output, exit));
    };
    let report: Report = match serde_json::from_str(json) {
        Ok(report) => report,
        Err(error) => return Parsed::crashed(format!("oxlint printed invalid JSON: {error}")),
    };
    if prose.contains(NO_FILES) {
        return Parsed::from_findings(Vec::new(), Some(0));
    }
    let files = report.number_of_files;
    let findings: Vec<ToolFinding> = report
        .diagnostics
        .into_iter()
        .map(|d| finding(d, context.cwd))
        .collect();
    if findings.is_empty() && files > 0 && exit != 0 {
        return Parsed::crashed(format!("oxlint exited {exit} but reported no diagnostics"));
    }
    Parsed::from_findings(findings, Some(files))
}

/// The prose before the report, and the report from its first line that opens with `{`.
fn split_json(stdout: &str) -> Option<(&str, &str)> {
    let mut offset = 0;
    for line in stdout.split_inclusive('\n') {
        if line.trim_start().starts_with('{') {
            return Some((stdout.get(..offset)?, stdout.get(offset..)?));
        }
        offset += line.len();
    }
    None
}

fn no_report_reason(output: &ToolOutput<'_>, exit: i32) -> String {
    let said = output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .map(str::trim)
        .find(|l| !l.is_empty());
    match said {
        Some(line) => format!("oxlint exited {exit} without a JSON report: {line}"),
        None => format!("oxlint exited {exit} without a JSON report"),
    }
}

fn finding(d: Diagnostic, cwd: &str) -> ToolFinding {
    let file = d.filename.map(|f| rebase(cwd, &f));
    let span = d.labels.first().map(|l| &l.span);
    let message = match d.help {
        Some(help) if !help.trim().is_empty() => format!("{} (help: {})", d.message, help.trim()),
        _ => d.message,
    };
    ToolFinding {
        severity: severity(&d.severity),
        rule: rule(d.code.as_deref()),
        fix: file
            .as_deref()
            .map(|f| format!("oxlint --fix {}", quote(f))),
        file,
        line: span.and_then(|s| nonzero(s.line)),
        column: span.and_then(|s| nonzero(s.column)),
        message,
    }
}

/// `eslint(no-debugger)` becomes `oxlint/eslint/no-debugger`.
pub(super) fn rule(code: Option<&str>) -> String {
    let Some(code) = code.map(str::trim).filter(|c| !c.is_empty()) else {
        return "oxlint/unknown".to_string();
    };
    let shaped = code
        .strip_suffix(')')
        .and_then(|c| c.split_once('('))
        .filter(|(plugin, name)| {
            !plugin.is_empty() && !name.is_empty() && !plugin.contains(['(', ')', ' '])
        });
    match shaped {
        Some((plugin, name)) => format!("oxlint/{plugin}/{name}"),
        None => format!("oxlint/{code}"),
    }
}

/// An unknown word counts as an error, so a new severity cannot pass the gate unseen.
fn severity(word: &str) -> Severity {
    match word {
        "warning" | "warn" => Severity::Warning,
        "advice" | "info" => Severity::Info,
        _ => Severity::Error,
    }
}
