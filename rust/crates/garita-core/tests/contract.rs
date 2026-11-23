use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use garita_core::receipt::{RECEIPT_VERSION, Receipt, ReceiptCheck, ResultWord, Tool};
use garita_core::report::{
    CheckEntry, CheckKind, CheckStatus, Finding, Report, RunContext, Scope, ScopeKind, Severity,
    Trigger,
};

const COMMIT: &str = "1b29de5ea3f04c7d9a6b2e8f5c1d0a7b3e9f4c62";
const TREE: &str = "9c1e4f2ab07d5e3c8f1a6b4d2e0c9f7a5b3d1e80";
const WORKTREE: &str = "4e7a1c9b2d5f8036a1b4c7e0d3f6a9c2b5e8d1f4";
const RUN_ID: &str = "01M7XM3Q4T8Z6R2K9V5B1N0HCW";
const STARTED: &str = "2026-11-21T03:15:02Z";
const FINISHED: &str = "2026-11-21T03:15:04Z";
const RECEIPT_PATH: &str = ".garita/receipts/20261121T031502Z-push-01M7XM3Q4T8Z6R2K9V5B1N0HCW.json";

fn scope() -> Scope {
    Scope {
        kind: ScopeKind::Affected,
        files: Some(vec!["src/app.ts".into(), "src/util.ts".into()]),
        packages: Some(vec!["frontend".into()]),
    }
}

fn context(receipt: bool) -> RunContext {
    RunContext {
        trigger: Some(Trigger::Push),
        commit: Some(COMMIT.into()),
        tree: Some(TREE.into()),
        scope: Some(scope()),
        receipt: receipt.then(|| RECEIPT_PATH.into()),
    }
}

fn entry(name: &str, kind: CheckKind, status: CheckStatus, files: u64) -> CheckEntry {
    CheckEntry {
        name: name.into(),
        kind,
        status,
        duration_ms: 412,
        examined: [("files".to_string(), files)].into(),
        rerun: Some(format!("garita run {name}")),
    }
}

fn finding(check: &str, severity: Severity, file: Option<&str>, line: Option<u64>) -> Finding {
    Finding {
        check: check.into(),
        severity,
        rule: format!("{check}/rule"),
        file: file.map(Into::into),
        line: line.and_then(std::num::NonZeroU64::new),
        column: line.and_then(|_| std::num::NonZeroU64::new(5)),
        message: "Unexpected `debugger` statement".into(),
        fix: None,
        fingerprint: Some("5d1c0e9a".into()),
    }
}

fn pass_report() -> Report {
    let checks = vec![
        entry("lint", CheckKind::Lint, CheckStatus::Pass, 38),
        entry("format", CheckKind::Format, CheckStatus::Pass, 38),
    ];
    Report::from_run(context(true), checks, Vec::new(), None)
}

fn warn_report() -> Report {
    let checks = vec![
        entry("lint", CheckKind::Lint, CheckStatus::Warn, 38),
        entry("format", CheckKind::Format, CheckStatus::Pass, 38),
    ];
    let findings = vec![finding(
        "lint",
        Severity::Warning,
        Some("src/app.ts"),
        Some(12),
    )];
    Report::from_run(context(true), checks, findings, None)
}

fn fail_report() -> Report {
    let checks = vec![
        entry("lint", CheckKind::Lint, CheckStatus::Fail, 38),
        entry("types", CheckKind::Types, CheckStatus::Pass, 38),
    ];
    let findings = vec![
        finding("lint", Severity::Error, Some("src/app.ts"), Some(1302)),
        finding("lint", Severity::Error, Some("src/util.ts"), Some(7)),
    ];
    Report::from_run(context(true), checks, findings, None)
}

fn not_run_report() -> Report {
    let context = RunContext {
        trigger: Some(Trigger::Commit),
        ..RunContext::default()
    };
    Report::not_run(context, "not a git repository".into())
}

fn mixed_not_run_report() -> Report {
    let mut missing = entry("types", CheckKind::Types, CheckStatus::NotRun, 0);
    missing.rerun = None;
    let checks = vec![
        entry("lint", CheckKind::Lint, CheckStatus::Fail, 38),
        missing,
    ];
    let findings = vec![finding(
        "lint",
        Severity::Error,
        Some("src/app.ts"),
        Some(3),
    )];
    Report::from_run(context(false), checks, findings, None)
}

fn no_location_report() -> Report {
    let checks = vec![entry("records", CheckKind::Records, CheckStatus::Fail, 4)];
    let findings = vec![finding("records", Severity::Error, None, None)];
    Report::from_run(context(true), checks, findings, None)
}

fn receipt_check(
    name: &str,
    kind: CheckKind,
    status: CheckStatus,
    code: Option<i32>,
) -> ReceiptCheck {
    ReceiptCheck {
        name: name.into(),
        kind,
        command: format!("{name} src/app.ts src/util.ts"),
        scope: scope(),
        tool_version: Some("1.2.3".into()),
        started_at: STARTED.into(),
        finished_at: FINISHED.into(),
        duration_ms: 412,
        exit_code: code,
        status,
        examined: [("files".to_string(), 38)].into(),
        output_tail: "2 errors".into(),
        output: "src/app.ts:1302:5 error\nsrc/util.ts:7:5 error\n2 errors".into(),
    }
}

fn receipt(result: ResultWord, checks: Vec<ReceiptCheck>, report: Report) -> Receipt {
    Receipt {
        receipt_version: RECEIPT_VERSION,
        run_id: RUN_ID.into(),
        started_at: STARTED.into(),
        finished_at: FINISHED.into(),
        trigger: Trigger::Push,
        commit: Some(COMMIT.parse().unwrap()),
        tree: TREE.parse().unwrap(),
        worktree: WORKTREE.parse().unwrap(),
        dirty: false,
        config_hash: "sha256:0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0"
            .into(),
        machine: "build-host".into(),
        tool: Tool {
            name: "garita".into(),
            version: "0.1.0".into(),
        },
        result,
        checks,
        report,
    }
}

fn json<T: serde::Serialize>(value: T) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

fn cases() -> Vec<(&'static str, serde_json::Value)> {
    let fail = fail_report();
    let fail_receipt = receipt(
        ResultWord::from(fail.status),
        vec![
            receipt_check("lint", CheckKind::Lint, CheckStatus::Fail, Some(1)),
            receipt_check("types", CheckKind::Types, CheckStatus::Pass, Some(0)),
        ],
        fail,
    );
    let mixed = mixed_not_run_report();
    let mixed_receipt = receipt(
        ResultWord::from(mixed.status),
        vec![
            receipt_check("lint", CheckKind::Lint, CheckStatus::Fail, Some(1)),
            receipt_check("types", CheckKind::Types, CheckStatus::NotRun, None),
        ],
        mixed,
    );
    let not_run = not_run_report();
    let not_run_receipt = receipt(ResultWord::from(not_run.status), Vec::new(), not_run);
    vec![
        ("gate-report.pass", json(pass_report())),
        ("gate-report.warn", json(warn_report())),
        ("gate-report.fail", json(fail_report())),
        ("gate-report.not-run-no-start", json(not_run_report())),
        ("gate-report.not-run-mixed", json(mixed_not_run_report())),
        (
            "gate-report.finding-no-location",
            json(no_location_report()),
        ),
        ("receipt.fail", json(fail_receipt)),
        ("receipt.not-run-mixed", json(mixed_receipt)),
        ("receipt.not-run", json(not_run_receipt)),
    ]
}

#[test]
fn examples_match_what_the_types_serialize() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/contract/examples");
    // GARITA_UPDATE_GOLDEN=1 rewrites the goldens instead of comparing, so a contract change is reviewed as a diff.
    let update = std::env::var("GARITA_UPDATE_GOLDEN").is_ok_and(|v| v == "1");
    if update {
        fs::create_dir_all(&dir).unwrap();
    }
    let mut produced = BTreeSet::new();
    for (name, value) in cases() {
        let file = format!("{name}.json");
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap());
        let path = dir.join(&file);
        if update {
            fs::write(&path, &text).unwrap();
        } else {
            let golden = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("golden {file} is missing: {e}"));
            assert_eq!(golden, text, "golden {file} differs");
        }
        produced.insert(file);
    }
    let on_disk: BTreeSet<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    if update {
        for stale in on_disk.difference(&produced) {
            fs::remove_file(dir.join(stale)).unwrap();
        }
    } else {
        assert_eq!(
            on_disk, produced,
            "examples/ holds files the test does not produce"
        );
    }
}

#[test]
fn a_receipt_and_its_report_agree_on_every_check() {
    let statuses = |checks: &serde_json::Value| -> Vec<(String, String)> {
        checks
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["name"].to_string(), c["status"].to_string()))
            .collect()
    };
    for (name, value) in cases() {
        if name.starts_with("receipt.") {
            assert_eq!(
                statuses(&value["checks"]),
                statuses(&value["report"]["checks"]),
                "{name}"
            );
        }
    }
}
