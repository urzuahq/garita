use super::{Outcome, ParseContext, Parsed, ToolFinding, ToolOutput, examined_files};
use crate::report::Severity;

const TAIL_LINES: usize = 20;
const TAIL_BYTES: usize = 4000;

/// Reads only the exit code, keeping the output's last lines as the finding's message.
pub fn parse(output: &ToolOutput<'_>, context: &ParseContext<'_>) -> Parsed {
    let Some(exit) = output.exit_code else {
        return Parsed::killed();
    };
    if exit == 0 {
        return Parsed::from_findings(Vec::new(), context.files);
    }
    let tail = tail(output.stderr, output.stdout);
    let message = if tail.is_empty() {
        format!("exited with code {exit} and printed nothing")
    } else {
        format!("exited with code {exit}:\n{tail}")
    };
    Parsed {
        outcome: Outcome::Findings,
        findings: vec![ToolFinding {
            severity: Severity::Error,
            rule: "exit-code/failed".to_string(),
            file: None,
            line: None,
            column: None,
            message,
            fix: None,
        }],
        examined: examined_files(context.files),
    }
}

/// The last non-blank lines of stderr then stdout, at most [`TAIL_LINES`] and [`TAIL_BYTES`].
pub(super) fn tail(stderr: &str, stdout: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let keep = lines.len().saturating_sub(TAIL_LINES);
    let joined = lines.get(keep..).unwrap_or_default().join("\n");
    let mut start = joined.len().saturating_sub(TAIL_BYTES);
    while !joined.is_char_boundary(start) {
        start += 1;
    }
    joined.get(start..).unwrap_or_default().to_string()
}
