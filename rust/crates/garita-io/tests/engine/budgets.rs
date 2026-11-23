use std::fs;

use garita_core::report::{CheckStatus, Severity, Trigger};

use crate::support::{Repo, check, finding, rules};

/// A check that sleeps for as long as the `delay` file says.
fn sleeper(repo: &Repo, check_extra: &str, top_extra: &str) {
    let delay = repo.base().join("delay");
    fs::write(&delay, "0.4").unwrap();
    let tool = repo.tool("slow", &format!("sleep \"$(cat '{}')\"", delay.display()));
    repo.config(&format!(
        "schema_version: 1\n{top_extra}checks:\n  slow:\n    run: {tool}\n{check_extra}"
    ));
}

fn set_delay(repo: &Repo, seconds: &str) {
    fs::write(repo.base().join("delay"), seconds).unwrap();
}

#[test]
fn over_budget_with_warn_is_a_budget_warning() {
    let repo = Repo::new();
    sleeper(&repo, "    budget: { duration: 150ms }\n", "");
    let out = repo.run();
    let f = finding(&out.report, "budget");
    assert_eq!(f.severity, Severity::Warning);
    assert!(
        f.message.contains("over its budget of 150ms"),
        "{}",
        f.message
    );
    assert_eq!(out.exit_code(), 0);
}

#[test]
fn over_budget_with_fail_is_an_error() {
    let repo = Repo::new();
    sleeper(
        &repo,
        "    budget: { duration: 150ms }\n    on_exceed: fail\n",
        "",
    );
    let out = repo.run();
    assert_eq!(finding(&out.report, "budget").severity, Severity::Error);
    assert_eq!(out.exit_code(), 1);
}

#[test]
fn demote_reports_then_skips_until_a_fast_forced_run() {
    let repo = Repo::new();
    sleeper(
        &repo,
        "    budget: { duration: 150ms }\n    on: [commit, ci]\n",
        "budgets: { on_exceed: demote }\n",
    );
    let first = repo.run();
    assert_eq!(finding(&first.report, "budget").severity, Severity::Warning);
    assert_eq!(check(&first.report, "slow").status, CheckStatus::Warn);

    set_delay(&repo, "0");
    let second = repo.run();
    assert_eq!(check(&second.report, "slow").status, CheckStatus::Skipped);
    let f = finding(&second.report, "demoted");
    assert_eq!(f.severity, Severity::Info);
    assert!(f.message.contains("demoted to ci"), "{}", f.message);
    assert!(
        f.message.contains("over its budget of 150ms"),
        "{}",
        f.message
    );

    let third = repo.run();
    assert_eq!(check(&third.report, "slow").status, CheckStatus::Skipped);

    let forced = repo.run_named(Trigger::Commit, &["slow"]);
    assert_eq!(check(&forced.report, "slow").status, CheckStatus::Pass);
    assert!(rules(&forced.report, "slow").is_empty());

    let after = repo.run();
    assert_eq!(check(&after.report, "slow").status, CheckStatus::Pass);
}

#[test]
fn a_forced_run_ignores_demotion_and_the_on_list() {
    let repo = Repo::new();
    sleeper(&repo, "    budget: { duration: 150ms }\n    on: [ci]\n", "");
    assert!(repo.run().report.checks.is_empty());
    set_delay(&repo, "0");
    let forced = repo.run_named(Trigger::Commit, &["slow"]);
    assert_eq!(check(&forced.report, "slow").status, CheckStatus::Pass);
}

#[test]
fn demote_without_a_later_trigger_acts_as_warn() {
    let repo = Repo::new();
    sleeper(
        &repo,
        "    budget: { duration: 150ms }\n    on: [commit]\n    on_exceed: demote\n",
        "",
    );
    repo.run();
    set_delay(&repo, "0");
    let second = repo.run();
    assert_eq!(check(&second.report, "slow").status, CheckStatus::Pass);
}

#[test]
fn a_trigger_over_its_budget_is_a_garita_budget_finding() {
    let repo = Repo::new();
    sleeper(
        &repo,
        "",
        "triggers:\n  commit: { budget: { duration: 150ms } }\n",
    );
    let out = repo.run();
    let f = finding(&out.report, "budget");
    assert_eq!(
        (f.check.as_str(), f.severity),
        ("garita", Severity::Warning)
    );
    assert!(f.message.contains("commit trigger"), "{}", f.message);
    assert_eq!(out.exit_code(), 0);
}

#[test]
fn a_trigger_over_budget_with_fail_blocks_and_reruns_the_trigger() {
    let repo = Repo::new();
    sleeper(
        &repo,
        "",
        "triggers:\n  commit: { budget: { duration: 150ms } }\nbudgets: { on_exceed: fail }\n",
    );
    let out = repo.run();
    assert_eq!(finding(&out.report, "budget").severity, Severity::Error);
    assert_eq!(out.exit_code(), 1);
    assert_eq!(
        out.report.next_action.as_deref(),
        Some("garita run --trigger commit")
    );
}
