use serde::Deserialize;

use super::{
    ParseContext, Parsed, ToolFinding, ToolOutput, examined_files, nonzero, oxlint_text, path_arg,
    rebase,
};
use crate::report::Severity;

const NO_FILES: &str = "No files found to lint";

#[derive(Deserialize)]
struct Report {
    diagnostics: Vec<Diagnostic>,
    number_of_files: u64,
}

#[derive(Deserialize)]
pub(super) struct Diagnostic {
    pub(super) message: String,
    #[serde(default)]
    pub(super) code: Option<String>,
    pub(super) severity: String,
    #[serde(default)]
    pub(super) help: Option<String>,
    #[serde(default)]
    pub(super) filename: Option<String>,
    #[serde(default)]
    pub(super) labels: Vec<Label>,
}

#[derive(Deserialize)]
pub(super) struct Label {
    pub(super) span: Span,
}

#[derive(Deserialize)]
pub(super) struct Span {
    pub(super) line: u64,
    pub(super) column: u64,
}

/// Parses oxlint's output: a JSON report (the first JSON value, which prose may precede or
/// follow), else its default text format in either of its styles.
pub fn parse(output: &ToolOutput<'_>, context: &ParseContext<'_>) -> Parsed {
    let Some(exit) = output.exit_code else {
        return Parsed::killed();
    };
    let report = split_json(output.stdout).and_then(|(prose, json)| {
        let first = serde_json::Deserializer::from_str(json)
            .into_iter::<Report>()
            .next();
        first.and_then(Result::ok).map(|report| (prose, report))
    });
    let Some((prose, report)) = report else {
        return parse_text(output, context, exit);
    };
    let files = report.number_of_files;
    let findings: Vec<ToolFinding> = report
        .diagnostics
        .into_iter()
        .map(|d| finding(d, context.cwd))
        .collect();
    if findings.is_empty() && prose.contains(NO_FILES) {
        return Parsed::from_findings(Vec::new(), Some(0));
    }
    if findings.is_empty() && files > 0 && exit != 0 {
        let mut parsed = Parsed::unrecognized("oxlint", exit, output);
        parsed.examined = examined_files(Some(files));
        return parsed;
    }
    Parsed::from_findings(findings, Some(files))
}

/// The text format cannot tell "not oxlint" from "nothing to report", so an exit of 0 without
/// diagnostics is the tool's own pass, and a non-zero exit without any still blocks.
fn parse_text(output: &ToolOutput<'_>, context: &ParseContext<'_>, exit: i32) -> Parsed {
    let (diagnostics, files) = oxlint_text::parse(output.stdout);
    let findings: Vec<ToolFinding> = diagnostics
        .into_iter()
        .map(|d| finding(d, context.cwd))
        .collect();
    if !findings.is_empty() {
        return Parsed::from_findings(findings, files);
    }
    if output.stdout.contains(NO_FILES) {
        return Parsed::from_findings(Vec::new(), Some(0));
    }
    if exit == 0 {
        return Parsed::from_findings(Vec::new(), files);
    }
    let mut parsed = Parsed::unrecognized("oxlint", exit, output);
    parsed.examined = examined_files(files);
    parsed
}

/// The prose before the report, and the text from its first line that opens with `{`.
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
            .map(|f| format!("oxlint --fix {}", path_arg(f))),
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
