use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use garita_core::adapter::rebase;
use garita_core::adapter::turbo::{clean_log, parse_summary};

const RUNS: &str = ".turbo/runs";

/// One task's output as its parser needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLog {
    pub task: String,
    /// Repository-relative.
    pub directory: String,
    pub log: String,
    pub exit_code: Option<i32>,
}

/// The run summaries present before turbo starts, so an older one is never read as this run's.
pub fn snapshot(cwd: &Path) -> BTreeSet<OsString> {
    fs::read_dir(cwd.join(RUNS))
        .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default()
}

/// Reads the summary this run wrote and each task's log; `Err` says what is missing.
pub fn read(
    cwd: &Path,
    cwd_rel: &str,
    before: &BTreeSet<OsString>,
) -> Result<Vec<TaskLog>, String> {
    let summary = newest_summary(cwd, before)?;
    let text = fs::read_to_string(&summary).map_err(|e| {
        format!(
            "could not read the turbo run summary {}: {e}",
            summary.display()
        )
    })?;
    let parsed = parse_summary(&text).map_err(|e| format!("{}: {e}", summary.display()))?;
    parsed
        .tasks
        .into_iter()
        .map(|task| {
            let Some(log_file) = task.log_file.as_deref() else {
                return Err(format!(
                    "turbo's summary names no log file for {}",
                    task.task_id
                ));
            };
            let path = cwd.join(log_file);
            let log = fs::read_to_string(&path).map_err(|e| {
                format!(
                    "the log of {} at {} cannot be read: {e}",
                    task.task_id,
                    path.display()
                )
            })?;
            Ok(TaskLog {
                directory: rebase(cwd_rel, &task.directory),
                log: clean_log(&log).to_string(),
                exit_code: task.exit_code,
                task: task.task_id,
            })
        })
        .collect()
}

fn newest_summary(cwd: &Path, before: &BTreeSet<OsString>) -> Result<PathBuf, String> {
    let dir = cwd.join(RUNS);
    let missing = || format!("turbo wrote no run summary under {}", dir.display());
    let entries = fs::read_dir(&dir).map_err(|_| missing())?;
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let json = Path::new(&name).extension().is_some_and(|e| e == "json");
        if !json || before.contains(&name) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(t, _)| modified > *t) {
            newest = Some((modified, entry.path()));
        }
    }
    newest.map(|(_, path)| path).ok_or_else(missing)
}
