//! The model of `.garita/config.yaml` (RFC-1 §3, RFC-2 §2).

mod duration;
mod parse;
mod version;

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use thiserror::Error;

use crate::argv::Command;
use crate::report::{CheckKind, ScopeKind, Severity, Trigger};

pub use duration::{DurationError, parse_duration};
pub const VIA_TURBO: &str = parse::VIA_TURBO;
pub use version::{Op, Version, VersionError, VersionReq, satisfies};

pub const CONFIG_SCHEMA_VERSION: u64 = 1;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
pub const DEFAULT_TIMEOUT_FACTOR: f64 = 10.0;
pub const DEFAULT_RECEIPTS_KEEP: u64 = 200;
pub const DEFAULT_PARSER: &str = "exit-code";

/// One problem in a config, located by its key path such as `checks.lint.on`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    #[error("the config is not valid YAML: {message}")]
    Syntax { message: String },
    #[error("{path}: unknown key; the keys allowed here are {allowed}")]
    UnknownKey { path: String, allowed: String },
    #[error("{path}: expected {expected}")]
    Type { path: String, expected: String },
    #[error("{path}: missing; {hint}")]
    Missing { path: String, hint: String },
    #[error("{path}: {message}")]
    Invalid { path: String, message: String },
}

impl ConfigError {
    pub fn path(&self) -> &str {
        match self {
            ConfigError::Syntax { .. } => "",
            ConfigError::UnknownKey { path, .. }
            | ConfigError::Type { path, .. }
            | ConfigError::Missing { path, .. }
            | ConfigError::Invalid { path, .. } => path,
        }
    }
}

/// Every problem found in a config, in document order.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub struct ConfigErrors(pub Vec<ConfigError>);

impl fmt::Display for ConfigErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<String> = self.0.iter().map(ToString::to_string).collect();
        f.write_str(&lines.join("; "))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnExceed {
    Warn,
    Fail,
    Demote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnCrash {
    Warn,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    Admin,
    Status,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceAccept {
    Ci,
    Local,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerConfig {
    pub scope: ScopeKind,
    pub budget: Option<Budget>,
    pub worktree: Worktree,
}

/// Where a trigger's checks run: the working tree, or a throwaway merge of the PR (RFC-1 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Worktree {
    Current,
    Merged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    CleanTree,
}

impl Builtin {
    pub const ALL: [Builtin; 1] = [Builtin::CleanTree];

    pub fn as_str(self) -> &'static str {
        match self {
            Builtin::CleanTree => "clean-tree",
        }
    }
}

impl TriggerConfig {
    pub fn default_for(trigger: Trigger) -> Self {
        let scope = match trigger {
            Trigger::Commit => ScopeKind::Staged,
            Trigger::Push => ScopeKind::Changed,
            Trigger::Ci | Trigger::Merge => ScopeKind::All,
        };
        Self {
            scope,
            budget: None,
            worktree: Worktree::Current,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Regression {
    pub window: Option<u64>,
    pub threshold_percent: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budgets {
    pub on_exceed: OnExceed,
    pub regression: Option<Regression>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConfig {
    pub mode: MergeMode,
    pub method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckForm {
    Run {
        run: String,
        command: Command,
        parser: String,
    },
    Native {
        tool: String,
    },
    Builtin {
        builtin: Builtin,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub form: CheckForm,
    pub globs: Vec<String>,
    pub exclude: Vec<String>,
    pub partition: Option<Partition>,
    pub via: Option<String>,
    /// Repository-relative, normalised, never empty or `.` (that is `None`).
    pub cwd: Option<String>,
    pub kind: Option<CheckKind>,
    pub budget: Option<Budget>,
    pub on_exceed: Option<OnExceed>,
    /// Sorted in trigger order (commit, push, ci, merge), whatever order the config lists.
    pub on: Vec<Trigger>,
    pub advisory: bool,
    /// Tool findings below this severity are dropped and counted, not reported.
    pub min_severity: Severity,
}

impl Check {
    /// The trigger a check demotes to from `from`: the next later one in its own `on` list.
    pub fn next_trigger(&self, from: Trigger) -> Option<Trigger> {
        self.on.iter().copied().find(|t| *t > from)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub requires: Option<VersionReq>,
    pub triggers: BTreeMap<Trigger, TriggerConfig>,
    pub budgets: Budgets,
    pub on_crash: OnCrash,
    pub timeout_factor: f64,
    pub receipts_keep: u64,
    pub merge: MergeConfig,
    pub evidence_accept: EvidenceAccept,
    pub checks: Vec<Check>,
}

impl Config {
    pub fn parse(yaml: &str) -> Result<Self, ConfigErrors> {
        parse::parse(yaml)
    }

    pub fn trigger(&self, trigger: Trigger) -> TriggerConfig {
        self.triggers
            .get(&trigger)
            .copied()
            .unwrap_or_else(|| TriggerConfig::default_for(trigger))
    }

    pub fn check(&self, name: &str) -> Option<&Check> {
        self.checks.iter().find(|c| c.name == name)
    }

    pub fn on_exceed(&self, check: &Check) -> OnExceed {
        check.on_exceed.unwrap_or(self.budgets.on_exceed)
    }

    /// `timeout.factor` times the check's budget, else its trigger's, else ten minutes (RFC-1 §4).
    pub fn timeout(&self, check: &Check, trigger: Trigger) -> Duration {
        let budget = check.budget.or(self.trigger(trigger).budget);
        match budget {
            Some(b) => Duration::try_from_secs_f64(b.duration.as_secs_f64() * self.timeout_factor)
                .unwrap_or(Duration::MAX),
            None => DEFAULT_TIMEOUT,
        }
    }
}

#[cfg(test)]
mod tests;
