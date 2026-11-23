//! The pure `parse` half of the built-in adapters (RFC-2 §1, §3).

use std::num::NonZeroU64;

use thiserror::Error;

use crate::report::{CheckKind, Examined, Finding, Severity};

mod exit_code;
mod fingerprint;
mod oxfmt;
mod oxlint;
mod path;
mod tsc;
pub mod turbo;

pub use fingerprint::fingerprint;
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
    pub fix: Option<String>,
}

impl ToolFinding {
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.rule, self.file.as_deref(), &self.message)
    }

    pub fn into_finding(self, check: &str) -> Finding {
        let fingerprint = Some(self.fingerprint());
        Finding {
            check: check.to_string(),
            severity: self.severity,
            rule: self.rule,
            file: self.file,
            line: self.line,
            column: self.column,
            message: self.message,
            fix: self.fix,
            fingerprint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub outcome: Outcome,
    pub findings: Vec<ToolFinding>,
    pub examined: Examined,
}

impl Parsed {
    fn crashed(reason: impl Into<String>) -> Self {
        Self {
            outcome: Outcome::crashed(reason),
            findings: Vec::new(),
            examined: Examined::new(),
        }
    }

    fn killed() -> Self {
        Self::crashed("the tool was killed or did not exit normally")
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
        }
    }
}

fn examined_files(files: Option<u64>) -> Examined {
    files
        .map(|n| Examined::from([(FILES.to_string(), n)]))
        .unwrap_or_default()
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
        match self {
            Adapter::Oxlint => oxlint::parse(output, context),
            Adapter::Oxfmt => oxfmt::parse(output, context),
            Adapter::Tsc => tsc::parse(output, context),
            Adapter::ExitCode => exit_code::parse(output, context),
        }
    }
}

#[cfg(test)]
mod tests;
