//! The pure `parse` half of the built-in adapters (RFC-2 §1, §3).

use std::borrow::Cow;
use std::num::NonZeroU64;

use thiserror::Error;

use crate::argv::join;
use crate::report::{CheckKind, Examined, Finding, Severity};

mod exit_code;
mod fingerprint;
mod invoke;
mod oxfmt;
mod oxlint;
mod oxlint_text;
mod path;
mod tsc;
pub mod turbo;

pub use exit_code::{is_noise, is_notable, last_lines, notable, printed_nothing, sanitize};
pub use fingerprint::fingerprint;
pub use invoke::complete_turbo_argv;
pub use path::rebase;
pub use tsc::count_project_files;

pub const FILES: &str = "files";

/// What a tool printed and how it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolOutput<'a> {
    pub stdout: &'a str,
    pub stderr: &'a str,
    /// `None` when the tool was killed or never exited normally.
    pub exit_code: Option<i32>,
}

/// Where and on what the tool ran.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseContext<'a> {
    /// The tool's working directory relative to the repository root, empty for the root.
    pub cwd: &'a str,
    /// The number of files passed to the tool, when known. For `tsc`, the project's file count
    /// from [`count_project_files`].
    pub files: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Clean,
    Findings,
    /// Ran on nothing: zero files examined, or the tool said no files matched.
    Nothing,
    /// The tool started but its output cannot be trusted.
    Crashed {
        reason: String,
    },
}

impl Outcome {
    pub fn name(&self) -> &'static str {
        match self {
            Outcome::Clean => "clean",
            Outcome::Findings => "findings",
            Outcome::Nothing => "nothing",
            Outcome::Crashed { .. } => "crashed",
        }
    }

    fn crashed(reason: impl Into<String>) -> Self {
        Outcome::Crashed {
            reason: reason.into(),
        }
    }
}

/// A finding as a parser sees it: the caller knows the check it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFinding {
    pub severity: Severity,
    pub rule: String,
    /// Repository-relative, with `/` separators.
    pub file: Option<String>,
    pub line: Option<NonZeroU64>,
    pub column: Option<NonZeroU64>,
    pub message: String,
    /// The command that fixes it, as its arguments.
    pub fix: Option<Vec<String>>,
}

impl ToolFinding {
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.rule, self.file.as_deref(), &self.message)
    }

    pub fn into_finding(self, check: &str) -> Finding {
        self.build(Some(check.to_string()))
    }

    /// A finding about the run itself, which no check produced.
    pub fn into_run_finding(self) -> Finding {
        self.build(None)
    }

    fn build(self, check: Option<String>) -> Finding {
        let fingerprint = Some(self.fingerprint());
        Finding {
            check,
            severity: self.severity,
            rule: self.rule,
            file: self.file,
            line: self.line,
            column: self.column,
            message: self.message,
            fix: self.fix.as_deref().map(join),
            fix_argv: self.fix,
            fingerprint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub outcome: Outcome,
    pub findings: Vec<ToolFinding>,
    pub examined: Examined,
    /// Lines of a failing output that look like its reason.
    pub notable: Vec<String>,
}

impl Parsed {
    fn crashed(reason: impl Into<String>) -> Self {
        Self {
            outcome: Outcome::crashed(reason),
            findings: Vec::new(),
            examined: Examined::new(),
            notable: Vec::new(),
        }
    }

    fn killed() -> Self {
        Self::crashed("the tool was killed or did not exit normally")
    }

    /// A non-zero exit the parser could not read still blocks: the tool gave its own verdict.
    fn unrecognized(tool: &str, exit: i32, output: &ToolOutput<'_>) -> Self {
        let message = if printed_nothing(output.stderr, output.stdout) {
            format!("{tool} exited with code {exit} and printed nothing")
        } else {
            format!("{tool} exited with code {exit} with output garita does not recognise")
        };
        Self {
            outcome: Outcome::Findings,
            findings: vec![ToolFinding {
                severity: Severity::Error,
                rule: format!("{tool}/unrecognized-output"),
                file: None,
                line: None,
                column: None,
                message,
                fix: None,
            }],
            examined: Examined::new(),
            notable: notable(output.stderr, output.stdout),
        }
    }

    /// A tool that exited non-zero blocks even when every finding it printed is below `error`,
    /// as a linter told to deny warnings does. Only "ran on nothing" and a crash are left alone.
    fn blocking_on_failure(mut self, tool: &str, exit: Option<i32>) -> Self {
        let failed = exit.filter(|code| *code != 0);
        let judged = matches!(self.outcome, Outcome::Clean | Outcome::Findings);
        let has_error = self.findings.iter().any(|f| f.severity == Severity::Error);
        if let (Some(code), true, false) = (failed, judged, has_error) {
            self.outcome = Outcome::Findings;
            self.findings.push(ToolFinding {
                severity: Severity::Error,
                rule: format!("{tool}/failed"),
                file: None,
                line: None,
                column: None,
                message: format!("{tool} exited with code {code} without reporting an error"),
                fix: None,
            });
        }
        self
    }

    /// `Findings` when there are any, else `Nothing` for zero files, else `Clean`.
    fn from_findings(findings: Vec<ToolFinding>, files: Option<u64>) -> Self {
        let outcome = if !findings.is_empty() {
            Outcome::Findings
        } else if files == Some(0) {
            Outcome::Nothing
        } else {
            Outcome::Clean
        };
        Self {
            outcome,
            findings,
            examined: examined_files(files),
            notable: Vec::new(),
        }
    }
}

fn examined_files(files: Option<u64>) -> Examined {
    files
        .map(|n| Examined::from([(FILES.to_string(), n)]))
        .unwrap_or_default()
}

const LINE_CAP: usize = 300;

/// At most [`LINE_CAP`] bytes of `text`, cut on a character boundary.
fn clip(text: &str) -> &str {
    let mut end = text.len().min(LINE_CAP);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// A repository path as a command argument: `./` keeps a leading `-` from reading as an option.
fn path_arg(file: &str) -> String {
    if file.starts_with('-') {
        format!("./{file}")
    } else {
        file.to_string()
    }
}

/// Removes ANSI escape sequences: CSI (`ESC [ ... final`), OSC (`ESC ] ... BEL` or `ESC \`),
/// and any other two-byte `ESC x`.
pub fn strip_ansi(text: &str) -> Cow<'_, str> {
    if !text.contains('\u{1b}') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    Cow::Owned(out)
}

fn nonzero(n: u64) -> Option<NonZeroU64> {
    NonZeroU64::new(n)
}

/// The built-in parsers, named as a check's `parser` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    Oxlint,
    Oxfmt,
    Tsc,
    ExitCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown parser `{name}`; known parsers: oxlint, oxfmt, tsc, exit-code")]
pub struct UnknownParser {
    pub name: String,
}

impl Adapter {
    pub const ALL: [Adapter; 4] = [
        Adapter::Oxlint,
        Adapter::Oxfmt,
        Adapter::Tsc,
        Adapter::ExitCode,
    ];

    pub fn from_parser_name(name: &str) -> Result<Self, UnknownParser> {
        Self::ALL
            .into_iter()
            .find(|a| a.name() == name)
            .ok_or_else(|| UnknownParser {
                name: name.to_string(),
            })
    }

    pub fn name(self) -> &'static str {
        match self {
            Adapter::Oxlint => "oxlint",
            Adapter::Oxfmt => "oxfmt",
            Adapter::Tsc => "tsc",
            Adapter::ExitCode => "exit-code",
        }
    }

    pub fn kind(self) -> CheckKind {
        match self {
            Adapter::Oxlint => CheckKind::Lint,
            Adapter::Oxfmt => CheckKind::Format,
            Adapter::Tsc => CheckKind::Types,
            Adapter::ExitCode => CheckKind::Other,
        }
    }

    pub fn parse(self, output: &ToolOutput<'_>, context: &ParseContext<'_>) -> Parsed {
        let stdout = strip_ansi(output.stdout);
        let stderr = strip_ansi(output.stderr);
        let output = &ToolOutput {
            stdout: &stdout,
            stderr: &stderr,
            exit_code: output.exit_code,
        };
        let parsed = match self {
            Adapter::Oxlint => oxlint::parse(output, context),
            Adapter::Oxfmt => oxfmt::parse(output, context),
            Adapter::Tsc => tsc::parse(output, context),
            Adapter::ExitCode => exit_code::parse(output, context),
        };
        parsed.blocking_on_failure(self.name(), output.exit_code)
    }
}

#[cfg(test)]
mod tests;
