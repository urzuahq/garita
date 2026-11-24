use std::fs;

use garita_core::receipt::{Receipt, ResultWord};
use garita_core::report::{Report, Severity, Trigger};

use crate::support::{Repo, check, finding};

fn two_check_repo() -> Repo {
    let repo = Repo::new();
    let ok = repo.tool("ok", "echo all good");
    let bad = repo.tool("bad", "echo it broke >&2; exit 4");
    repo.write("a.ts", "a\n");
    repo.stage(&["a.ts"]);
    repo.config(&format!(
        "schema_version: 1\nreceipts: {{ keep: 2 }}\nchecks:\n  ok:\n    run: {ok} {{files}}\n  bad:\n    run: {bad}\n  \
         none:\n    run: {ok} {{files}}\n    globs: ['*.md']\n"
    ));
    repo
}

#[test]
fn a_receipt_is_written_and_agrees_with_the_report() {
    let repo = two_check_repo();
    let out = repo.run();
    let receipts = repo.receipts();
    assert_eq!(receipts.len(), 1);
    let receipt: Receipt =
        serde_json::from_str(&fs::read_to_string(&receipts[0]).unwrap()).unwrap();
    assert_eq!(receipt.report, out.report);
    assert_eq!(receipt.result, ResultWord::Failed);
    assert_eq!(receipt.trigger, Trigger::Commit);
    assert_eq!(receipt.commit, None);
    assert_eq!(receipt.tool.name, "garita");
    assert_eq!(receipt.tool.version, "0.1.0");
    assert!(receipt.dirty);
    assert_ne!(receipt.worktree.as_str(), receipt.tree.as_str());
    assert_eq!(Some(receipt.tree.as_str()), out.report.tree.as_deref());
    assert_eq!(receipt.checks.len(), out.report.checks.len());
    for (entry, rc) in out.report.checks.iter().zip(&receipt.checks) {
        assert_eq!(
            (&rc.name, rc.kind, rc.status),
            (&entry.name, entry.kind, entry.status)
        );
        assert_eq!(rc.duration_ms, entry.duration_ms);
        assert_eq!(rc.examined, entry.examined);
        assert!(repo.root().join(&rc.output).is_file(), "{}", rc.output);
    }
    let ok = &receipt.checks[0];
    assert_eq!(ok.exit_code, Some(0));
    assert!(ok.command.ends_with("/ok a.ts"), "{}", ok.command);
    assert_eq!(ok.output_tail, "all good\n");
    let bad = &receipt.checks[1];
    assert_eq!(bad.exit_code, Some(4));
    assert!(bad.output_tail.contains("it broke"));
    let none = &receipt.checks[2];
    assert_eq!(none.exit_code, None);
    assert!(none.output_tail.contains("no file in scope"));
    let name = receipts[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.contains("-commit-") && name.contains(&receipt.run_id),
        "{name}"
    );
    assert_eq!(
        out.report.receipt.as_deref(),
        Some(&*format!(".garita/receipts/{name}"))
    );
}

#[test]
fn receipts_are_pruned_to_keep() {
    let repo = two_check_repo();
    for _ in 0..4 {
        repo.run();
    }
    assert_eq!(repo.receipts().len(), 2);
    let dirs = fs::read_dir(repo.root().join(".garita/receipts"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
        .count();
    assert_eq!(dirs, 2);
}

#[test]
fn a_receipt_that_cannot_be_written_leaves_the_verdict_standing() {
    let repo = two_check_repo();
    fs::create_dir_all(repo.root().join(".garita")).unwrap();
    fs::write(repo.root().join(".garita/receipts"), "not a directory").unwrap();
    let out = repo.run();
    assert_eq!(out.report.receipt, None);
    let f = finding(&out.report, "receipt-unwritten");
    assert_eq!((f.check.as_deref(), f.severity), (None, Severity::Info));
    assert!(
        f.message.contains("could not write the receipt"),
        "{}",
        f.message
    );
    assert_eq!(out.exit_code(), 1);
    assert_eq!(out.report.status, garita_core::report::RunStatus::Fail);
}

fn single_check_repo(script: &str) -> Repo {
    let repo = Repo::new();
    let tool = repo.tool("noisy", script);
    repo.write("a.ts", "a\n");
    repo.stage(&["a.ts"]);
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  noisy:\n    run: {tool}\n"
    ));
    repo
}

fn is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|o| o.stdout.starts_with(b"0"))
}

#[test]
fn output_tail_and_notable_lines_carry_no_control_characters() {
    let repo = single_check_repo(
        "printf 'esc\\033]0;title\\007 \\033[31mred\\033[0m nul:\\000 cr:\\r bell error: bad\\n'; exit 1",
    );
    let out = repo.run();
    let entry = check(&out.report, "noisy");
    let tail = entry.output_tail.as_deref().unwrap();
    assert!(tail.contains("red"), "{tail:?}");
    let bad = |c: char| c.is_control() && c != '\n' && c != '\t';
    assert!(!tail.chars().any(bad), "{tail:?}");
    assert!(!entry.output_notable.iter().any(|l| l.chars().any(bad)));
    assert!(!entry.output_notable.is_empty());
    let receipt = repo.receipt_of(&out.report);
    let in_receipt = receipt.checks.iter().find(|c| c.name == "noisy").unwrap();
    assert_eq!(in_receipt.output_tail, tail);
}

#[cfg(unix)]
#[test]
fn an_unwritable_receipts_directory_still_yields_the_tail() {
    use std::os::unix::fs::PermissionsExt;
    if is_root() {
        return;
    }
    let repo = single_check_repo("echo boom-detail; exit 1");
    let dir = repo.root().join(".garita/receipts");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let out = repo.run();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    let entry = check(&out.report, "noisy");
    assert!(
        entry
            .output_tail
            .as_deref()
            .is_some_and(|t| t.contains("boom-detail")),
        "{:?}",
        entry.output_tail
    );
    assert_eq!(entry.output, None);
    assert_eq!(out.exit_code(), 1);
    assert_eq!(out.report.status, garita_core::report::RunStatus::Fail);
    finding(&out.report, "receipt-unwritten");
}

#[test]
fn an_engine_report_can_be_saved_and_read_back() {
    let repo = two_check_repo();
    let out = repo.run();
    let path = repo.base().join("report.json");
    fs::write(&path, garita_io::engine::to_json(&out.report) + "\n").unwrap();
    let back: Report = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(back, out.report);
}

#[test]
fn consecutive_runs_on_an_unchanged_repository_share_a_worktree_hash() {
    let repo = two_check_repo();
    repo.write(".gitignore", "");
    repo.commit_all("everything");
    let first = repo.run();
    let second = repo.run();
    let (a, b) = (
        repo.receipt_of(&first.report),
        repo.receipt_of(&second.report),
    );
    assert_eq!(a.worktree, b.worktree);
    assert!(!b.dirty);
}

#[test]
fn reruns_after_a_push_failure_name_the_trigger() {
    let repo = two_check_repo();
    repo.commit_all("everything");
    let out = repo.run_at(Trigger::Ci);
    assert_eq!(
        out.report.next_action.as_deref(),
        Some("garita run --trigger ci bad")
    );
    let bad = out.report.checks.iter().find(|c| c.name == "bad").unwrap();
    assert_eq!(bad.rerun.as_deref(), Some("garita run --trigger ci bad"));
    let none = out.report.checks.iter().find(|c| c.name == "none").unwrap();
    assert_eq!(none.rerun.as_deref(), Some("garita run --trigger ci none"));
}

#[test]
fn a_batched_runs_output_tail_is_the_first_failing_batch_not_the_last() {
    let repo = Repo::new();
    let tool = repo.tool(
        "lint",
        "for f; do case \"$f\" in *target*) echo \"bad $f\" >&2; exit 1;; esac; done\necho \"$@\"\nexit 0",
    );
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  lint:\n    run: {tool} {{files}}\n"
    ));
    let dir = "d".repeat(180);
    for i in 0..1800 {
        let name = if i == 5 {
            format!("{dir}/target.ts")
        } else {
            format!("{dir}/f{i:05}.ts")
        };
        repo.write(&name, "");
    }
    repo.stage(&[&dir]);
    let out = repo.run();
    assert_eq!(out.exit_code(), 1);
    let receipt = repo.receipt_of(&out.report);
    let lint = receipt.checks.iter().find(|c| c.name == "lint").unwrap();
    assert!(lint.output_tail.contains("bad "), "{}", lint.output_tail);
    assert!(lint.output_tail.contains("target.ts"));
    let log = fs::read_to_string(repo.root().join(&lint.output)).unwrap();
    assert_eq!(log.matches("==> ").count(), log.matches("<==").count());
    assert!(log.matches("==> ").count() >= 3);
    assert!(log.trim_end().ends_with(".ts"));
}
