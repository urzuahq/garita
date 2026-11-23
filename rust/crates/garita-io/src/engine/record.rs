use std::path::Path;

use garita_core::ObjectId;
use garita_core::receipt::{RECEIPT_VERSION, Receipt, ReceiptCheck, ResultWord, Tool};
use garita_core::report::{Report, Scope, ScopeKind, Trigger};

use super::judge::CheckResult;
use crate::clock;

/// Everything a receipt says about the run besides its checks and report.
pub struct RunFacts<'a> {
    pub root: &'a Path,
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

pub fn receipt(facts: &RunFacts<'_>, results: &[CheckResult], report: &Report) -> Receipt {
    let real_root = std::fs::canonicalize(facts.root).unwrap_or_else(|_| facts.root.to_path_buf());
    let checks = results
        .iter()
        .map(|r| {
            let output = r.record.output.as_deref().map_or_else(String::new, |p| {
                p.strip_prefix(&real_root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            });
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
