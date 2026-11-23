#![cfg(unix)]

mod common;

use std::fs;
use std::path::Path;

use common::{TempDir, init, write};
use garita_core::receipt::{RECEIPT_VERSION, Receipt, ReceiptCheck, ResultWord, Tool};
use garita_core::report::{
    CheckKind, CheckStatus, Examined, Report, RunContext, Scope, ScopeKind, Trigger,
};
use garita_io::{clock, receipts};

const TREE: &str = "9c1e4f2ab07d5e3c8f1a6b4d2e0c9f7a5b3d1e80";

fn check(name: &str, status: CheckStatus) -> ReceiptCheck {
    ReceiptCheck {
        name: name.into(),
        kind: CheckKind::Other,
        command: name.into(),
        scope: Scope {
            kind: ScopeKind::All,
            files: None,
            packages: None,
        },
        tool_version: None,
        started_at: "2026-11-21T03:15:02Z".into(),
        finished_at: "2026-11-21T03:15:02Z".into(),
        duration_ms: 1,
        exit_code: Some(0),
        status,
        examined: Examined::new(),
        output_tail: String::new(),
        output: String::new(),
    }
}

fn receipt(minute: u32, trigger: Trigger, checks: Vec<ReceiptCheck>) -> Receipt {
    let at = format!("2026-11-21T03:{minute:02}:00Z");
    Receipt {
        receipt_version: RECEIPT_VERSION,
        run_id: clock::run_id(),
        started_at: at.clone(),
        finished_at: at,
        trigger,
        commit: None,
        tree: TREE.parse().unwrap(),
        worktree: TREE.parse().unwrap(),
        dirty: false,
        config_hash: "x".into(),
        machine: "host".into(),
        tool: Tool {
            name: "garita".into(),
            version: "0.0.0".into(),
        },
        result: ResultWord::Passed,
        checks,
        report: Report::not_run(RunContext::default(), "x".into()),
    }
}

use CheckStatus::{Pass, Skipped};

/// Oldest first: r[0] .. r[4].
fn history(root: &Path) -> Vec<Receipt> {
    let all = vec![
        receipt(
            1,
            Trigger::Commit,
            vec![check("lint", Pass), check("types", Pass)],
        ),
        receipt(
            2,
            Trigger::Commit,
            vec![check("lint", Pass), check("types", Skipped)],
        ),
        receipt(3, Trigger::Push, vec![check("lint", Pass)]),
        receipt(
            4,
            Trigger::Commit,
            vec![check("lint", Pass), check("types", Skipped)],
        ),
        receipt(
            5,
            Trigger::Commit,
            vec![check("lint", Skipped), check("types", Skipped)],
        ),
    ];
    for r in &all {
        receipts::write(root, r).unwrap();
        let out = receipts::output_path(root, &r.run_id, "lint").unwrap();
        fs::write(out, "log").unwrap();
    }
    all
}

fn ids(listing: &receipts::Listing) -> Vec<String> {
    listing.receipts.iter().map(|s| s.run_id.clone()).collect()
}

#[test]
fn write_names_the_file_and_returns_a_relative_path() {
    let tmp = TempDir::new();
    let r = receipt(15, Trigger::Push, vec![]);
    let rel = receipts::write(tmp.path(), &r).unwrap();
    assert_eq!(
        rel,
        format!(".garita/receipts/20261121T031500Z-push-{}.json", r.run_id)
    );
    let text = fs::read_to_string(tmp.path().join(&rel)).unwrap();
    assert_eq!(serde_json::from_str::<Receipt>(&text).unwrap(), r);
    let names: Vec<_> = fs::read_dir(tmp.path().join(receipts::DIR))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "no temporary file is left: {names:?}");
}

#[test]
fn output_path_sits_in_the_runs_directory() {
    let tmp = TempDir::new();
    let id = clock::run_id();
    let path = receipts::output_path(tmp.path(), &id, "lint").unwrap();
    assert_eq!(
        path,
        tmp.path().join(receipts::DIR).join(&id).join("lint.log")
    );
    assert!(path.parent().unwrap().is_dir());
    for bad in ["../x", "a/b", "", ".hidden"] {
        assert!(
            receipts::output_path(tmp.path(), &id, bad).is_err(),
            "{bad}"
        );
    }
    assert!(receipts::output_path(tmp.path(), "../../etc", "lint").is_err());
}

#[test]
fn list_is_newest_first_and_reports_bad_files() {
    let tmp = TempDir::new();
    let root = tmp.path();
    assert!(receipts::list(root).unwrap().receipts.is_empty());
    let all = history(root);
    let dir = root.join(receipts::DIR);
    fs::write(
        dir.join(format!("20261121T039900Z-ci-{}.json", clock::run_id())),
        "{ not json",
    )
    .unwrap();
    fs::write(dir.join("notes.json"), "{}").unwrap();
    fs::write(dir.join("README"), "ignored").unwrap();
    let listing = receipts::list(root).unwrap();
    let want: Vec<String> = all.iter().rev().map(|r| r.run_id.clone()).collect();
    assert_eq!(ids(&listing), want);
    assert_eq!(listing.skipped.len(), 2, "{:?}", listing.skipped);
}

#[test]
fn last_run_of_skips_receipts_where_the_check_was_skipped() {
    let tmp = TempDir::new();
    let root = tmp.path();
    let all = history(root);
    let id = |t, c| receipts::last_run_of(root, c, t).unwrap().map(|s| s.run_id);
    assert_eq!(id(Trigger::Commit, "lint"), Some(all[3].run_id.clone()));
    assert_eq!(id(Trigger::Commit, "types"), Some(all[0].run_id.clone()));
    assert_eq!(id(Trigger::Push, "lint"), Some(all[2].run_id.clone()));
    assert_eq!(id(Trigger::Ci, "lint"), None);
    assert_eq!(id(Trigger::Commit, "nope"), None);
}

#[test]
fn prune_keeps_the_newest_and_the_last_run_of_each_check() {
    let tmp = TempDir::new();
    let root = tmp.path();
    let all = history(root);
    let dir = root.join(receipts::DIR);
    let corrupt = dir.join(format!("20261121T030000Z-ci-{}.json", clock::run_id()));
    fs::write(&corrupt, "garbage").unwrap();
    let pruned = receipts::prune(root, 2).unwrap();
    assert_eq!(pruned.deleted.len(), 1);
    assert_eq!(pruned.kept_for_demotion.len(), 2);
    assert_eq!(pruned.skipped.len(), 1);
    assert!(corrupt.exists());
    let left = ids(&receipts::list(root).unwrap());
    let want: Vec<String> = [4, 3, 2, 0]
        .iter()
        .map(|i| all[*i].run_id.clone())
        .collect();
    assert_eq!(left, want);
    assert!(
        !dir.join(&all[1].run_id).exists(),
        "its output went with it"
    );
    assert!(dir.join(&all[0].run_id).join("lint.log").exists());
    assert_eq!(receipts::prune(root, 2).unwrap().deleted.len(), 0);
}

#[test]
fn prune_never_follows_a_symlink_out_of_the_store() {
    let tmp = TempDir::new();
    let outside = TempDir::new();
    fs::write(outside.path().join("victim"), "keep me").unwrap();
    let root = tmp.path();
    let all = history(root);
    let dir = root.join(receipts::DIR);
    let out_dir = dir.join(&all[1].run_id);
    fs::remove_dir_all(&out_dir).unwrap();
    std::os::unix::fs::symlink(outside.path(), &out_dir).unwrap();
    let linked = dir.join(format!("20261121T030000Z-ci-{}.json", clock::run_id()));
    std::os::unix::fs::symlink(outside.path().join("victim"), &linked).unwrap();
    receipts::prune(root, 0).unwrap();
    assert!(
        fs::symlink_metadata(&out_dir).is_err(),
        "the link itself is removed"
    );
    assert!(
        linked.exists(),
        "a linked receipt is not read, so not pruned"
    );
    assert_eq!(
        fs::read_to_string(outside.path().join("victim")).unwrap(),
        "keep me"
    );
}

#[test]
fn a_store_that_links_outside_the_repository_is_refused() {
    let tmp = TempDir::new();
    let outside = TempDir::new();
    fs::create_dir_all(tmp.path().join(".garita")).unwrap();
    std::os::unix::fs::symlink(outside.path(), tmp.path().join(receipts::DIR)).unwrap();
    let r = receipt(1, Trigger::Ci, vec![]);
    assert!(matches!(
        receipts::write(tmp.path(), &r),
        Err(receipts::ReceiptsError::OutsideRepository(_))
    ));
    assert!(receipts::prune(tmp.path(), 0).is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn reports_whether_git_ignores_the_store() {
    let tmp = TempDir::new();
    let root = tmp.path();
    init(root);
    assert!(!receipts::ignored_by_git(root).unwrap());
    write(root, ".gitignore", ".garita/receipts/\n");
    assert!(receipts::ignored_by_git(root).unwrap());
    write(root, ".gitignore", "/.garita/\n");
    assert!(receipts::ignored_by_git(root).unwrap());
    write(root, ".gitignore", "");
    write(root, ".garita/.gitignore", "receipts/\n");
    assert!(receipts::ignored_by_git(root).unwrap());
}
