//! The pure half of running a tool through turbo (RFC-2 §5).

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("the turbo run summary cannot be read: {0}")]
pub struct SummaryError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CacheStatus {
    Hit,
    Miss,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub task_id: String,
    pub task: String,
    pub package: String,
    /// Relative to the workspace root.
    pub directory: String,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    /// Relative to the workspace root.
    pub log_file: Option<String>,
    pub cache: CacheStatus,
    /// Milliseconds since the Unix epoch.
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
}

impl Task {
    /// A cache hit replays an earlier run's log: the tool did not run this time.
    pub fn cache_hit(&self) -> bool {
        self.cache == CacheStatus::Hit
    }

    pub fn ran_fresh(&self) -> bool {
        !self.cache_hit() && self.exit_code.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub exit_code: Option<i32>,
    /// How many tasks turbo says it attempted; tasks stopped early may be missing from `tasks`.
    pub attempted: Option<u64>,
    pub tasks: Vec<Task>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSummary {
    #[serde(default)]
    execution: Option<RawRun>,
    tasks: Vec<RawTask>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRun {
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    attempted: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTask {
    task_id: String,
    task: String,
    package: String,
    directory: String,
    #[serde(default)]
    log_file: Option<String>,
    cache: RawCache,
    #[serde(default)]
    execution: Option<RawExecution>,
}

#[derive(Deserialize)]
struct RawCache {
    status: CacheStatus,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawExecution {
    #[serde(default)]
    start_time: Option<u64>,
    #[serde(default)]
    end_time: Option<u64>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    error: Option<String>,
}

/// Reads the JSON `turbo run --summarize` writes (a `--dry=json` plan reads the same way, with
/// no exit codes).
pub fn parse_summary(json: &str) -> Result<Summary, SummaryError> {
    let raw: RawSummary = serde_json::from_str(json).map_err(|e| SummaryError(e.to_string()))?;
    let tasks = raw
        .tasks
        .into_iter()
        .map(|t| {
            let execution = t.execution;
            Task {
                task_id: t.task_id,
                task: t.task,
                package: t.package,
                directory: t.directory,
                exit_code: execution.as_ref().and_then(|e| e.exit_code),
                error: execution.as_ref().and_then(|e| e.error.clone()),
                log_file: t.log_file,
                cache: t.cache.status,
                start_time: execution.as_ref().and_then(|e| e.start_time),
                end_time: execution.as_ref().and_then(|e| e.end_time),
            }
        })
        .collect();
    Ok(Summary {
        exit_code: raw.execution.as_ref().and_then(|e| e.exit_code),
        attempted: raw.execution.and_then(|e| e.attempted),
        tasks,
    })
}

/// A task's log without the package manager's lines: its echo of the command (the first line)
/// and its closing failure line. The tool's own lines are returned untouched.
pub fn clean_log(log: &str) -> &str {
    let mut body = log;
    if body.starts_with("$ ") {
        body = body.split_once('\n').map_or("", |(_, rest)| rest);
    }
    let trimmed = body.trim_end();
    match trimmed.rsplit_once('\n') {
        Some((head, last)) if is_failure_line(last) => body.get(..=head.len()).unwrap_or(head),
        None if is_failure_line(trimmed) => "",
        _ => body,
    }
}

fn is_failure_line(line: &str) -> bool {
    let line = line.trim();
    let rest = line
        .strip_prefix("[ELIFECYCLE]")
        .or_else(|| line.strip_prefix("ELIFECYCLE"));
    rest.is_some_and(|r| r.trim_start().starts_with("Command failed"))
}
