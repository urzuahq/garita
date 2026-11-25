//! `garita run` end to end: config, scope, checks, verdict and receipt (RFC-1 §2–§5, §8).

mod exec;
pub(crate) mod finding;
mod judge;
mod plan;
mod record;
pub(crate) mod scope;
mod select;
pub(crate) mod session;
mod turbo;

use std::path::PathBuf;
use std::time::SystemTime;

use garita_core::report::{Report, RunContext, RunError, Trigger};

/// The time source for a run's start and finish.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// What `garita run` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Where the run starts; the repository root is found from here.
    pub dir: PathBuf,
    /// The running garita's version, checked against the config's `requires`.
    pub version: String,
    pub trigger: Trigger,
    pub base: Option<String>,
    /// Repository-relative files that replace the trigger's scope.
    pub files: Option<Vec<String>>,
    /// Checks named on the command line: run exactly these.
    pub checks: Vec<String>,
}

impl Request {
    pub fn new(dir: PathBuf, version: &str, trigger: Trigger) -> Self {
        Self {
            dir,
            version: version.to_string(),
            trigger,
            base: None,
            files: None,
            checks: Vec::new(),
        }
    }
}

/// The report to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub report: Report,
}

impl Outcome {
    fn not_run(trigger: Trigger, tree: Option<String>, error: RunError) -> Self {
        let context = RunContext {
            trigger: Some(trigger),
            tree,
            ..RunContext::default()
        };
        Self {
            report: Report::not_run(context, error),
        }
    }

    pub fn exit_code(&self) -> u8 {
        self.report.exit_code()
    }
}

/// Runs the gate. Every failure garita cannot attribute to a check is a `not-run` report.
pub fn run(request: &Request, clock: &dyn Clock) -> Outcome {
    session::run(request, clock)
}

/// The report as one line of compact JSON.
pub fn to_json(report: &Report) -> String {
    serde_json::to_string(report).unwrap_or_else(|e| {
        let error = serde_json::Value::String(format!("could not serialize the report: {e}"));
        format!(
            "{{\"schema_version\":1,\"status\":\"not-run\",\"blocking\":false,\"trigger\":null,\
             \"commit\":null,\"tree\":null,\"scope\":null,\"checks\":[],\"findings\":[],\
             \"examined\":{{}},\"next_action\":\"garita doctor\",\
             \"next_action_reason\":\"the run could not start\",\
             \"next_action_argv\":[\"garita\",\"doctor\"],\"next_action_inputs\":[],\
             \"receipt\":null,\"error\":{error},\"error_kinds\":[\"internal\"]}}"
        )
    })
}
