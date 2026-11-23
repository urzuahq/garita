//! The record of one run (RFC-1 §8).

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::report::{CheckKind, CheckStatus, Examined, Report, RunStatus, Scope, Trigger};

pub const RECEIPT_VERSION: u32 = 1;
pub const OUTPUT_TAIL_CHARS: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ResultWord {
    Passed,
    Warned,
    Failed,
}

impl From<RunStatus> for ResultWord {
    fn from(status: RunStatus) -> Self {
        match status {
            RunStatus::Pass => ResultWord::Passed,
            RunStatus::Warn => ResultWord::Warned,
            RunStatus::Fail | RunStatus::NotRun => ResultWord::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptCheck {
    pub name: String,
    pub kind: CheckKind,
    pub command: String,
    pub scope: Scope,
    pub tool_version: Option<String>,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    /// The tool's own exit code; `None` when it was killed or never started.
    pub exit_code: Option<i32>,
    pub status: CheckStatus,
    pub examined: Examined,
    pub output_tail: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub receipt_version: u32,
    pub run_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub trigger: Trigger,
    pub commit: Option<String>,
    pub tree: String,
    pub worktree: String,
    pub dirty: bool,
    pub config_hash: String,
    pub machine: String,
    pub tool: Tool,
    pub result: ResultWord,
    pub checks: Vec<ReceiptCheck>,
    pub report: Report,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReceiptNameError {
    #[error("`{0}` is not an RFC 3339 UTC time such as 2026-11-21T03:15:02Z")]
    Time(String),
    #[error("`{0}` is not a ULID")]
    RunId(String),
}

/// The receipt's file name, `<UTC timestamp>-<trigger>-<run id>.json`, from an RFC 3339 UTC time.
pub fn file_name(
    started_at: &str,
    trigger: Trigger,
    run_id: &str,
) -> Result<String, ReceiptNameError> {
    if !is_ulid(run_id) {
        return Err(ReceiptNameError::RunId(run_id.to_string()));
    }
    let stamp =
        compact_utc(started_at).ok_or_else(|| ReceiptNameError::Time(started_at.to_string()))?;
    Ok(format!("{stamp}-{}-{run_id}.json", trigger.as_str()))
}

/// `2026-11-21T03:15:02.5Z` becomes `20261121T031502Z`; sub-second digits are dropped.
fn compact_utc(time: &str) -> Option<String> {
    let rest = time.strip_suffix('Z')?;
    let (date, clock) = rest.split_once('T')?;
    let (clock, fraction) = clock.split_once('.').unwrap_or((clock, "0"));
    let digits_in = |part: &str, lens: &[usize], sep: char| {
        let pieces: Vec<&str> = part.split(sep).collect();
        let ok = pieces.len() == lens.len()
            && pieces
                .iter()
                .zip(lens)
                .all(|(p, n)| p.len() == *n && p.bytes().all(|b| b.is_ascii_digit()));
        ok.then(|| pieces.concat())
    };
    let date = digits_in(date, &[4, 2, 2], '-')?;
    let clock = digits_in(clock, &[2, 2, 2], ':')?;
    let fraction_ok = !fraction.is_empty() && fraction.bytes().all(|b| b.is_ascii_digit());
    fraction_ok.then(|| format!("{date}T{clock}Z"))
}

fn is_ulid(id: &str) -> bool {
    const ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    id.len() == 26
        && id.starts_with(|c: char| ('0'..='7').contains(&c))
        && id.chars().all(|c| ALPHABET.contains(c))
}

/// The last [`OUTPUT_TAIL_CHARS`] characters of a check's output.
pub fn output_tail(output: &str) -> &str {
    let count = output.chars().count();
    if count <= OUTPUT_TAIL_CHARS {
        return output;
    }
    let skip = count - OUTPUT_TAIL_CHARS;
    match output.char_indices().nth(skip) {
        Some((at, _)) => &output[at..],
        None => output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{RunContext, ScopeKind};

    const RUN_ID: &str = "01M7XM3Q4T8Z6R2K9V5B1N0HCW";
    const TREE: &str = "9c1e4f2ab07d5e3c8f1a6b4d2e0c9f7a5b3d1e80";

    fn receipt(report: Report) -> Receipt {
        Receipt {
            receipt_version: RECEIPT_VERSION,
            run_id: RUN_ID.into(),
            started_at: "2026-11-21T03:15:02Z".into(),
            finished_at: "2026-11-21T03:15:03Z".into(),
            trigger: Trigger::Commit,
            commit: None,
            tree: TREE.into(),
            worktree: TREE.into(),
            dirty: false,
            config_hash: "abc".into(),
            machine: "host".into(),
            tool: Tool {
                name: "garita".into(),
                version: "0.1.0".into(),
            },
            result: report.status.into(),
            checks: vec![ReceiptCheck {
                name: "lint".into(),
                kind: CheckKind::Lint,
                command: "oxlint a.ts".into(),
                scope: Scope {
                    kind: ScopeKind::Staged,
                    files: Some(vec!["a.ts".into()]),
                    packages: None,
                },
                tool_version: None,
                started_at: "2026-11-21T03:15:02.100Z".into(),
                finished_at: "2026-11-21T03:15:02.400Z".into(),
                duration_ms: 300,
                exit_code: None,
                status: CheckStatus::Pass,
                examined: Examined::from([("files".to_string(), 1)]),
                output_tail: String::new(),
                output: ".garita/receipts/x/lint.log".into(),
            }],
            report,
        }
    }

    #[test]
    fn result_words_follow_the_status() {
        assert_eq!(ResultWord::from(RunStatus::Pass), ResultWord::Passed);
        assert_eq!(ResultWord::from(RunStatus::Warn), ResultWord::Warned);
        assert_eq!(ResultWord::from(RunStatus::Fail), ResultWord::Failed);
        assert_eq!(ResultWord::from(RunStatus::NotRun), ResultWord::Failed);
    }

    #[test]
    fn serializes_every_field_the_schema_requires() {
        let r = receipt(Report::not_run(RunContext::default(), "x".into()));
        let value = serde_json::to_value(&r).unwrap();
        for key in [
            "receipt_version",
            "run_id",
            "started_at",
            "finished_at",
            "trigger",
            "commit",
            "tree",
            "worktree",
            "dirty",
            "config_hash",
            "machine",
            "tool",
            "result",
            "checks",
            "report",
        ] {
            assert!(value.get(key).is_some(), "{key}");
        }
        assert_eq!(value["result"], "FAILED");
        assert_eq!(value["commit"], serde_json::Value::Null);
        let check = &value["checks"][0];
        for key in [
            "name",
            "kind",
            "command",
            "scope",
            "tool_version",
            "started_at",
            "finished_at",
            "duration_ms",
            "exit_code",
            "status",
            "examined",
            "output_tail",
            "output",
        ] {
            assert!(check.get(key).is_some(), "{key}");
        }
        assert_eq!(check["exit_code"], serde_json::Value::Null);
        assert_eq!(check["tool_version"], serde_json::Value::Null);
        let back: Receipt = serde_json::from_value(value).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn file_name_follows_the_rfc_example() {
        assert_eq!(
            file_name("2026-11-21T03:15:02Z", Trigger::Push, RUN_ID).unwrap(),
            "20261121T031502Z-push-01M7XM3Q4T8Z6R2K9V5B1N0HCW.json"
        );
        assert_eq!(
            file_name("2026-11-21T03:15:02.987654Z", Trigger::Commit, RUN_ID).unwrap(),
            "20261121T031502Z-commit-01M7XM3Q4T8Z6R2K9V5B1N0HCW.json"
        );
    }

    #[test]
    fn file_name_rejects_non_utc_or_malformed_times() {
        for bad in [
            "2026-11-21T03:15:02+01:00",
            "2026-11-21 03:15:02Z",
            "2026-11-21T03:15Z",
            "26-11-21T03:15:02Z",
            "2026-11-21T03:15:02.Z",
            "2026-1a-21T03:15:02Z",
            "",
        ] {
            assert_eq!(
                file_name(bad, Trigger::Ci, RUN_ID),
                Err(ReceiptNameError::Time(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn file_name_rejects_a_bad_run_id() {
        for bad in [
            "",
            "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
            "01M7XM3Q4T8Z6R2K9V5B1N0HCI",
            "../x",
        ] {
            assert_eq!(
                file_name("2026-11-21T03:15:02Z", Trigger::Ci, bad),
                Err(ReceiptNameError::RunId(bad.into()))
            );
        }
    }

    #[test]
    fn output_tail_keeps_the_last_characters() {
        assert_eq!(output_tail("short"), "short");
        let long = format!("{}{}", "é".repeat(10), "x".repeat(OUTPUT_TAIL_CHARS));
        assert_eq!(output_tail(&long), "x".repeat(OUTPUT_TAIL_CHARS));
        let mixed = "é".repeat(OUTPUT_TAIL_CHARS + 1);
        assert_eq!(output_tail(&mixed).chars().count(), OUTPUT_TAIL_CHARS);
    }
}
