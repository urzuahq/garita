use std::path::Path;

use garita_core::ObjectId;
use garita_core::receipt::{RECEIPT_VERSION, Receipt, ReceiptCheck, ResultWord, Tool};
use garita_core::report::{CheckStatus, Report, Scope, ScopeKind, Trigger};

use super::judge::CheckResult;
use crate::clock;

/// Everything a receipt says about the run besides its checks and report.
pub struct RunFacts<'a> {
    pub run_id: &'a str,
    pub started_at: &'a str,
    pub finished_at: &'a str,
    pub trigger: Trigger,
    pub commit: Option<ObjectId>,
    pub tree: ObjectId,
    pub worktree: ObjectId,
    pub dirty: bool,
    pub config_hash: &'a str,
    pub version: &'a str,
    pub scope: ScopeKind,
}

/// Puts each check's output path, and the tail of a check that did not pass, on its report entry,
/// from the same values its receipt check holds.
pub fn attach_output(root: &Path, results: &mut [CheckResult]) {
    let real_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    for r in results {
        r.entry.output = r.record.output.as_deref().map(|p| {
            p.strip_prefix(&real_root)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        });
        let shown = !matches!(r.entry.status, CheckStatus::Pass | CheckStatus::Skipped);
        r.entry.output_tail = (shown && !r.record.tail.is_empty()).then(|| r.record.tail.clone());
    }
}

pub fn receipt(facts: &RunFacts<'_>, results: &[CheckResult], report: &Report) -> Receipt {
    let checks = results
        .iter()
        .map(|r| {
            let output = r.entry.output.clone().unwrap_or_default();
            ReceiptCheck {
                name: r.entry.name.clone(),
                kind: r.entry.kind,
                command: r.record.command.clone(),
                scope: Scope {
                    kind: facts.scope,
                    files: Some(r.record.files.clone()),
                    packages: None,
                },
                tool_version: None,
                started_at: clock::rfc3339(r.record.started),
                finished_at: clock::rfc3339(r.record.finished),
                duration_ms: r.entry.duration_ms,
                exit_code: r.record.exit_code,
                status: r.entry.status,
                examined: r.entry.examined.clone(),
                output_tail: r.record.tail.clone(),
                output,
            }
        })
        .collect();
    Receipt {
        receipt_version: RECEIPT_VERSION,
        run_id: facts.run_id.to_string(),
        started_at: facts.started_at.to_string(),
        finished_at: facts.finished_at.to_string(),
        trigger: facts.trigger,
        commit: facts.commit.clone(),
        tree: facts.tree.clone(),
        worktree: facts.worktree.clone(),
        dirty: facts.dirty,
        config_hash: facts.config_hash.to_string(),
        machine: clock::machine(),
        tool: Tool {
            name: "garita".to_string(),
            version: facts.version.to_string(),
        },
        result: ResultWord::from(report.status),
        checks,
        report: report.clone(),
    }
}
