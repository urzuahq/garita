use garita_core::report::{
    CheckStatus, MatchedBy, RequiredCheck, Risk, RiskSource, RunStatus, Severity, Trigger,
};

use crate::support::{Repo, finding, git_in, run_rules};

const POLICY: &str = "risk:
  classes:
    - { name: routine, approvals: 0 }
    - { name: core }
    - { name: guarded, author_counts: true }
  default: core
  match:
    - { paths: [\"docs/**\"], class: routine }
    - { paths: [\"crates/core/**\"], class: core }
    - { paths: [\"crates/signing/**\"], class: guarded }
";

fn config(repo: &Repo, risk: &str) -> String {
    let ok = repo.tool("ok", "exit 0");
    format!("schema_version: 1\n{risk}checks:\n  ok:\n    run: {ok}\n")
}

/// `main` holds `base_risk`; the checked-out `feature` branch adds `files`.
fn branched(base_risk: &str, files: &[&str]) -> Repo {
    let repo = Repo::new();
    repo.config(&config(&repo, base_risk));
    repo.write("README.md", "r\n");
    repo.write("crates/signing/k.rs", "k\n");
    repo.commit_all("base");
    repo.git(&["switch", "-q", "-c", "feature"]);
    for file in files {
        repo.write(file, "x\n");
    }
    repo
}

fn push(repo: &Repo) -> (RunStatus, Option<Risk>) {
    repo.commit_all("change");
    let out = repo.run_at(Trigger::Push);
    let risk = out.report.risk.clone().expect("a run report carries risk");
    let receipt = repo.receipt_of(&out.report);
    assert_eq!(receipt.report.risk, out.report.risk, "the receipt holds it");
    (out.report.status, risk)
}

fn by(path: &str, pattern: &str, class: &str) -> MatchedBy {
    MatchedBy {
        path: path.into(),
        pattern: pattern.into(),
        class: class.into(),
    }
}

#[test]
fn risk_is_null_when_the_base_declares_no_policy() {
    let repo = branched("", &["crates/core/a.rs"]);
    let (status, risk) = push(&repo);
    assert_eq!((status, risk), (RunStatus::Pass, None));
    let json = garita_io::engine::to_json(&repo.run_at(Trigger::Push).report);
    assert!(json.contains("\"risk\":null"), "{json}");
}

#[test]
fn risk_is_null_without_a_config_at_the_base() {
    let repo = Repo::new();
    repo.write("a.ts", "a\n");
    repo.commit_all("base");
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.config(&config(&repo, POLICY));
    let (_, risk) = push(&repo);
    assert_eq!(risk, None);
}

#[test]
fn a_matched_class_reports_its_properties_and_required_checks() {
    let repo = branched(POLICY, &["crates/core/a.rs", "docs/a.md"]);
    let (status, risk) = push(&repo);
    assert_eq!(status, RunStatus::Pass);
    let risk = risk.unwrap();
    assert_eq!(risk.class.as_deref(), Some("core"));
    assert_eq!(risk.source, RiskSource::Matched);
    assert_eq!(risk.enforced, Some(true));
    assert_eq!(risk.requires, Some(0));
    assert_eq!((risk.reached, risk.met), (Some(0), Some(true)));
    assert_eq!((risk.approvals, risk.author_counts), (Some(1), Some(false)));
    assert!(risk.needs_person);
    assert_eq!(
        risk.matched_by,
        [by("crates/core/a.rs", "crates/core/**", "core")]
    );
    assert_eq!(
        risk.required,
        [RequiredCheck {
            check: "ok".into(),
            system: 0,
            status: Some(CheckStatus::Pass),
        }]
    );
}

#[test]
fn a_routine_change_needs_no_person() {
    let repo = branched(POLICY, &["docs/a.md"]);
    let risk = push(&repo).1.unwrap();
    assert_eq!(risk.class.as_deref(), Some("routine"));
    assert_eq!(risk.approvals, Some(0));
    assert!(!risk.needs_person);
}

#[test]
fn an_unmatched_change_gets_the_default() {
    let repo = branched(POLICY, &["src/a.ts"]);
    let risk = push(&repo).1.unwrap();
    assert_eq!(risk.class.as_deref(), Some("core"));
    assert_eq!(risk.source, RiskSource::Default);
    assert!(risk.matched_by.is_empty());
}

#[test]
fn a_failing_required_check_leaves_the_class_unmet() {
    let repo = branched(POLICY, &["docs/a.md"]);
    let fail = repo.tool("fail", "exit 1");
    let mut head = config(&repo, POLICY);
    head.push_str(&format!(
        "  fail:\n    run: {fail}\n  advice:\n    run: {fail}\n    advisory: true\n"
    ));
    repo.config(&head);
    let risk = push(&repo).1.unwrap();
    assert_eq!((risk.reached, risk.met), (None, Some(false)));
    let names: Vec<&str> = risk.required.iter().map(|r| r.check.as_str()).collect();
    assert_eq!(names, ["ok", "fail"]);
}

#[test]
fn a_head_that_deletes_the_policy_is_still_judged_by_the_base() {
    let repo = branched(POLICY, &["docs/a.md"]);
    repo.config(&config(&repo, ""));
    let risk = push(&repo).1.unwrap();
    assert_eq!(risk.class.as_deref(), Some("guarded"));
    assert_eq!(
        risk.matched_by,
        [by(".garita/config.yaml", ".garita/**", "guarded")]
    );
}

#[test]
fn a_head_that_lowers_its_class_is_still_judged_by_the_base() {
    let repo = branched(POLICY, &[]);
    repo.git(&["mv", "crates/signing/k.rs", "k.rs"]);
    let lowered = "risk:\n  classes: [{ name: routine, approvals: 0 }]\n";
    repo.config(&config(&repo, lowered));
    repo.commit_all("lower");
    let mut request = repo.request(Trigger::Push);
    request.base = Some("main".into());
    let risk = repo.run_request(&request).report.risk.unwrap().unwrap();
    assert_eq!(risk.class.as_deref(), Some("guarded"));
    assert!(
        risk.matched_by
            .contains(&by("crates/signing/k.rs", "crates/signing/**", "guarded")),
        "{risk:?}"
    );
}

#[test]
fn at_commit_the_staged_change_is_judged() {
    let repo = branched(POLICY, &["docs/a.md", "crates/core/a.rs"]);
    repo.stage(&["docs/a.md"]);
    let risk = repo.run().report.risk.unwrap().unwrap();
    assert_eq!(risk.class.as_deref(), Some("routine"));
    repo.stage(&["crates/core/a.rs"]);
    let risk = repo.run().report.risk.unwrap().unwrap();
    assert_eq!(risk.class.as_deref(), Some("core"));
}

#[test]
fn an_invalid_base_policy_is_unreadable_and_the_run_goes_on() {
    let broken = "risk:\n  classes: [{ name: core, requires: 1 }]\n";
    let repo = branched(broken, &["docs/a.md"]);
    repo.config(&config(&repo, POLICY));
    repo.commit_all("repair");
    let out = repo.run_at(Trigger::Push);
    assert_eq!(out.report.status, RunStatus::Pass);
    assert_eq!(out.report.checks.len(), 1);
    let risk = out.report.risk.clone().unwrap().unwrap();
    assert_eq!(risk.class, None);
    assert_eq!(risk.source, RiskSource::BaseUnreadable);
    assert!(risk.needs_person);
    assert_eq!((risk.reached, risk.met), (Some(0), Some(true)));
    let f = finding(&out.report, "risk-unreadable");
    assert_eq!(f.severity, Severity::Info);
    assert!(f.message.contains("requires"), "{}", f.message);
    assert!(f.message.contains("a person must approve"), "{}", f.message);
}

#[test]
fn a_shallow_clone_without_the_merge_base_is_unreadable() {
    let origin = branched(POLICY, &["crates/core/a.rs"]);
    origin.commit_all("change");
    origin.git(&["switch", "-q", "main"]);
    origin.write("docs/b.md", "b\n");
    origin.commit_all("ahead");
    let url = format!("file://{}", origin.root().display());
    let clone = origin.base().join("clone");
    git_in(
        origin.base(),
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            "--no-single-branch",
            "--branch",
            "feature",
            &url,
            clone.to_str().unwrap(),
        ],
    );
    let repo = origin.adopt("clone");
    let out = repo.run_at(Trigger::Push);
    let risk = out.report.risk.clone().unwrap().unwrap();
    assert_eq!(risk.source, RiskSource::BaseUnreadable);
    assert!(run_rules(&out.report).contains(&"risk-unreadable"));
    assert_eq!(out.report.status, RunStatus::Pass);
}

#[test]
fn with_no_base_ref_a_head_without_a_policy_has_none() {
    let repo = Repo::new();
    repo.config(&config(&repo, ""));
    repo.commit_all("only");
    let out = repo.run_at(Trigger::Push);
    assert_eq!(out.report.risk, Some(None));
    assert!(!run_rules(&out.report).contains(&"risk-unreadable"));
}

#[test]
fn with_no_base_ref_a_head_with_a_policy_is_unreadable() {
    let repo = Repo::new();
    repo.config(&config(&repo, POLICY));
    repo.commit_all("only");
    let out = repo.run_at(Trigger::Push);
    let risk = out.report.risk.clone().unwrap().unwrap();
    assert_eq!(risk.source, RiskSource::BaseUnreadable);
}

#[test]
fn a_report_that_could_not_start_carries_no_risk() {
    let repo = Repo::new();
    repo.config("schema_version: 9\n");
    let out = repo.run_at(Trigger::Push);
    assert_eq!(out.report.status, RunStatus::NotRun);
    assert_eq!(out.report.risk, None);
    let json = garita_io::engine::to_json(&out.report);
    assert!(!json.contains("\"risk\""), "{json}");
}
