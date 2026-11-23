//! oxlint's default text output: the graphical style (a marker line, a `,-[file:line:col]`
//! frame and a `help:` line) and the one-line style it prints when it detects an agent.

use super::oxlint::{Diagnostic, Label, Span};

/// The diagnostics in `stdout`, and the file count from its closing `Finished … on N files`.
/// A marker line with neither a rule nor a location (a bad config) is not a diagnostic: the
/// unrecognised-output finding keeps all of its lines.
pub(super) fn parse(stdout: &str) -> (Vec<Diagnostic>, Option<u64>) {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut open = false;
    let mut files = None;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(n) = finished_files(trimmed) {
            files = Some(n);
            open = false;
        } else if let Some(d) = header(trimmed).or_else(|| one_line(trimmed)) {
            open = d.labels.is_empty();
            diagnostics.push(d);
        } else if let (true, Some(d)) = (open, diagnostics.last_mut()) {
            if let Some((file, span)) = frame(trimmed) {
                if d.filename.is_none() {
                    d.filename = Some(file.to_string());
                    d.labels.push(Label { span });
                }
            } else if let Some(help) = trimmed.strip_prefix("help:") {
                d.help = Some(help.trim().to_string());
                open = false;
            }
        }
    }
    diagnostics.retain(|d| d.code.is_some() || d.filename.is_some());
    (diagnostics, files)
}

/// `x code(name): message` for an error, `! …` for a warning.
fn header(line: &str) -> Option<Diagnostic> {
    let (severity, rest) = [
        ("x ", "error"),
        ("× ", "error"),
        ("! ", "warning"),
        ("⚠ ", "warning"),
    ]
    .into_iter()
    .find_map(|(marker, severity)| line.strip_prefix(marker).map(|rest| (severity, rest)))?;
    let (code, message) = split_code(rest.trim());
    Some(Diagnostic {
        message: message.to_string(),
        code,
        severity: severity.to_string(),
        help: None,
        filename: None,
        labels: Vec::new(),
    })
}

/// `file:line:col: severity code(name): message help: help`.
fn one_line(line: &str) -> Option<Diagnostic> {
    let (location, rest) = line.split_once(": ")?;
    let (file, span) = location_of(location)?;
    let (severity, rest) = rest.split_once(' ')?;
    if !matches!(severity, "error" | "warning" | "advice") {
        return None;
    }
    let (code, message) = split_code(rest.trim());
    let (message, help) = match message.rsplit_once(" help: ") {
        Some((message, help)) => (message, Some(help.trim().to_string())),
        None => (message, None),
    };
    Some(Diagnostic {
        message: message.to_string(),
        code,
        severity: severity.to_string(),
        help,
        filename: Some(file.to_string()),
        labels: vec![Label { span }],
    })
}

/// A leading `plugin(rule): ` as the code, when the message has one.
fn split_code(text: &str) -> (Option<String>, &str) {
    match text.split_once(": ") {
        Some((code, message))
            if code.ends_with(')') && code.contains('(') && !code.contains(' ') =>
        {
            (Some(code.to_string()), message)
        }
        _ => (None, text),
    }
}

/// `,-[file:line:col]`.
fn frame(line: &str) -> Option<(&str, Span)> {
    let inner = line.strip_prefix(",-[")?.strip_suffix(']')?;
    location_of(inner)
}

fn location_of(text: &str) -> Option<(&str, Span)> {
    let mut parts = text.rsplitn(3, ':');
    let column = parts.next()?.parse().ok()?;
    let line = parts.next()?.parse().ok()?;
    let file = parts.next().filter(|f| !f.is_empty())?;
    Some((file, Span { line, column }))
}

/// `Finished in 4ms on 3 files …`.
fn finished_files(line: &str) -> Option<u64> {
    let rest = line.strip_prefix("Finished in ")?;
    let (_, after) = rest.split_once(" on ")?;
    let (count, unit) = after.split_once(' ')?;
    unit.starts_with("file")
        .then(|| count.parse().ok())
        .flatten()
}
