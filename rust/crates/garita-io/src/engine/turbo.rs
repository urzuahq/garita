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

/// Reads the summary this run wrote and each task's log; `Err` says what is missing. Only a
/// summary that holds a task `argv` names counts, so another check's turbo run is never read.
pub fn read(
    cwd: &Path,
    cwd_rel: &str,
    before: &BTreeSet<OsString>,
    argv: &[String],
) -> Result<Vec<TaskLog>, String> {
    let wanted = task_names(argv);
    let dir = cwd.join(RUNS);
    let mut parsed = None;
    for path in new_summaries(&dir, before) {
        let text = fs::read_to_string(&path).map_err(|e| {
            format!(
                "could not read the turbo run summary {}: {e}",
                path.display()
            )
        })?;
        let summary = parse_summary(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let ours = wanted.is_empty()
            || summary
                .tasks
                .iter()
                .any(|t| wanted.contains(&t.task) || wanted.contains(&t.task_id));
        if ours {
            parsed = Some(summary);
            break;
        }
    }
    let Some(parsed) = parsed else {
        let tasks = if wanted.is_empty() {
            String::new()
        } else {
            format!(" for {}", wanted.join(", "))
        };
        return Err(format!(
            "turbo wrote no run summary{tasks} under {}",
            dir.display()
        ));
    };
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

/// The tasks a turbo command names: the words after `turbo` (and `run`) up to `--` that are not
/// flags. A flag's value may be among them, which only widens the match.
pub fn task_names(argv: &[String]) -> Vec<String> {
    let Some(at) = argv
        .iter()
        .position(|a| Path::new(a).file_stem().is_some_and(|stem| stem == "turbo"))
    else {
        return Vec::new();
    };
    argv.iter()
        .skip(at + 1)
        .take_while(|a| *a != "--")
        .filter(|a| !a.starts_with('-') && *a != "run")
        .cloned()
        .collect()
}

/// This run's summaries, newest first.
fn new_summaries(dir: &Path, before: &BTreeSet<OsString>) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            Path::new(&name).extension().is_some_and(|e| e == "json") && !before.contains(&name)
        })
        .map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (modified, entry.path())
        })
        .collect();
    found.sort_by(|a, b| b.cmp(a));
    found.into_iter().map(|(_, path)| path).collect()
}

#[cfg(test)]
mod tests {
    use super::task_names;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn task_names_are_the_words_after_turbo_up_to_the_separator() {
        assert_eq!(
            task_names(&argv(&[
                "pnpm",
                "turbo",
                "run",
                "lint",
                "types",
                "--continue",
                "--",
                "x"
            ])),
            argv(&["lint", "types"])
        );
        assert_eq!(
            task_names(&argv(&["/bin/turbo", "lint:check", "--summarize"])),
            argv(&["lint:check"])
        );
        assert!(task_names(&argv(&["pnpm", "lint"])).is_empty());
    }
}
