use super::*;

const RFC1_EXAMPLE: &str = r#"
schema_version: 1
requires: ">=0.1.0"          # the oldest garita that may run this config

triggers:
  commit: { scope: staged,   budget: { duration: 3s } }
  push:   { scope: affected, budget: { duration: 60s } }
  ci:     { scope: all,      budget: { duration: 10m } }
  merge:  { scope: all, worktree: merged }

budgets:
  on_exceed: demote          # warn | fail | demote
  regression: { window: 20, threshold: 25% }

on_crash: warn              # warn | fail
timeout: { factor: 10 }     # a check is killed at this multiple of its budget
receipts: { keep: 200 }

merge:
  mode: status               # admin | status | auto
  method: squash

evidence:
  accept: ci                 # ci | local | any

checks:
  lint:
    run: oxlint {files}
    parser: oxlint
    globs: ["**/*.{ts,tsx}"]
    budget: { duration: 1s }
    on: [commit, push, ci]
  format:
    run: oxfmt --check {files}
    parser: oxfmt
    budget: { duration: 1s }
    on: [commit, push, ci]
  types:
    run: turbo type-check {affected}
    parser: tsc
    via: turbo
    budget: { duration: 20s }
    on: [push, ci]
  test:
    run: turbo test {affected}
    parser: vitest
    via: turbo
    budget: { duration: 45s }
    on: [push, ci]
  records:
    native: urzua
    budget: { duration: 1s }
    on: [commit, ci]
  clean-tree:
    builtin: clean-tree
    on: [push, ci, merge]
"#;

const RFC2_EXAMPLE: &str = r#"
schema_version: 1
checks:
  lint:
    run: oxlint {files}
    parser: oxlint
    partition: { by: dir, roots: ["packages/*"] }
    exclude: ["packages/docs/**"]
  types:
    run: turbo type-check {affected}
    parser: tsc
    via: turbo
  records:
    native: urzua
"#;

fn ok(yaml: &str) -> Config {
    match Config::parse(yaml) {
        Ok(c) => c,
        Err(e) => panic!("{e}"),
    }
}

fn errs(yaml: &str) -> Vec<ConfigError> {
    Config::parse(yaml).unwrap_err().0
}

fn paths(yaml: &str) -> Vec<String> {
    errs(yaml).iter().map(|e| e.path().to_string()).collect()
}

fn with_check(body: &str) -> String {
    format!("schema_version: 1\nchecks:\n  c:\n{body}")
}

#[test]
fn rfc1_example_parses_as_written() {
    let c = ok(RFC1_EXAMPLE);
    assert_eq!(c.trigger(Trigger::Merge).worktree, Worktree::Merged);
    assert_eq!(c.trigger(Trigger::Push).worktree, Worktree::Current);
    let clean = c.check("clean-tree").unwrap();
    assert_eq!(
        clean.form,
        CheckForm::Builtin {
            builtin: Builtin::CleanTree
        }
    );
    assert_eq!(clean.on, [Trigger::Push, Trigger::Ci, Trigger::Merge]);
    assert_eq!(
        c.requires.as_ref().map(|r| r.text.as_str()),
        Some(">=0.1.0")
    );
    assert_eq!(
        c.trigger(Trigger::Push),
        TriggerConfig {
            scope: ScopeKind::Affected,
            budget: Some(Budget {
                duration: Duration::from_secs(60)
            }),
            worktree: Worktree::Current,
        }
    );
    assert_eq!(
        c.trigger(Trigger::Ci).budget.map(|b| b.duration),
        Some(Duration::from_secs(600))
    );
    assert_eq!(c.trigger(Trigger::Merge).budget, None);
    assert_eq!(c.budgets.on_exceed, OnExceed::Demote);
    assert_eq!(
        c.budgets.regression,
        Some(Regression {
            window: Some(20),
            threshold_percent: Some(25.0)
        })
    );
    assert_eq!(c.on_crash, OnCrash::Warn);
    assert_eq!(c.timeout_factor, 10.0);
    assert_eq!(c.receipts_keep, 200);
    assert_eq!(c.merge.mode, MergeMode::Status);
    assert_eq!(c.merge.method.as_deref(), Some("squash"));
    assert_eq!(c.evidence_accept, EvidenceAccept::Ci);
    let names: Vec<&str> = c.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["lint", "format", "types", "test", "records", "clean-tree"]
    );
    let lint = c.check("lint").unwrap();
    assert_eq!(lint.globs, ["**/*.{ts,tsx}"]);
    assert!(matches!(&lint.form, CheckForm::Run { parser, .. } if parser == "oxlint"));
    assert_eq!(c.check("types").unwrap().via.as_deref(), Some("turbo"));
    assert_eq!(
        c.check("records").unwrap().form,
        CheckForm::Native {
            tool: "urzua".into()
        }
    );
    assert_eq!(
        c.check("records").unwrap().on,
        [Trigger::Commit, Trigger::Ci]
    );
}

#[test]
fn rfc2_example_parses() {
    let c = ok(RFC2_EXAMPLE);
    let lint = c.check("lint").unwrap();
    assert_eq!(
        lint.partition,
        Some(Partition {
            roots: vec!["packages/*".into()]
        })
    );
    assert_eq!(lint.exclude, ["packages/docs/**"]);
    assert_eq!(lint.on, Trigger::ALL);
    assert_eq!(c.check("types").unwrap().via.as_deref(), Some("turbo"));
    assert!(matches!(
        c.check("records").unwrap().form,
        CheckForm::Native { .. }
    ));
}

#[test]
fn schema_version_alone_is_a_valid_config_with_defaults() {
    let c = ok("schema_version: 1\n");
    assert_eq!(c.requires, None);
    assert_eq!(c.trigger(Trigger::Commit).scope, ScopeKind::Staged);
    assert_eq!(c.trigger(Trigger::Push).scope, ScopeKind::Changed);
    assert_eq!(c.trigger(Trigger::Ci).scope, ScopeKind::All);
    assert_eq!(c.trigger(Trigger::Merge).scope, ScopeKind::All);
    assert_eq!(c.budgets.on_exceed, OnExceed::Warn);
    assert_eq!(c.budgets.regression, None);
    assert_eq!(c.on_crash, OnCrash::Warn);
    assert_eq!(c.timeout_factor, DEFAULT_TIMEOUT_FACTOR);
    assert_eq!(c.receipts_keep, DEFAULT_RECEIPTS_KEEP);
    assert_eq!(
        c.merge,
        MergeConfig {
            mode: MergeMode::Status,
            method: None
        }
    );
    assert_eq!(c.evidence_accept, EvidenceAccept::Ci);
    assert!(c.checks.is_empty());
}

#[test]
fn schema_version_is_required_and_must_be_one() {
    assert_eq!(paths("checks: {}\n"), ["schema_version"]);
    assert!(matches!(errs("")[..], [ConfigError::Type { .. }]));
    assert_eq!(paths("schema_version: 2\n"), ["schema_version"]);
    assert_eq!(paths("schema_version: '1'\n"), ["schema_version"]);
    let message = errs("schema_version: 2\n")[0].to_string();
    assert!(message.contains("not 2"), "{message}");
}

#[test]
fn invalid_yaml_is_a_syntax_error() {
    assert!(matches!(
        errs("schema_version: [1\n")[..],
        [ConfigError::Syntax { .. }]
    ));
}

#[test]
fn unknown_keys_are_errors_at_every_level() {
    let yaml = r#"
schema_version: 1
risk: high
triggers: { deploy: {}, push: { scope: all, extra: 1 } }
budgets: { on_exceed: warn, x: 1 }
timeout: { factor: 2, y: 1 }
receipts: { keep: 3, z: 1 }
merge: { w: 1 }
evidence: { v: 1 }
checks:
  c:
    run: x
    u: 1
    budget: { duration: 1s, files: 3 }
    partition: { by: dir, roots: [a], t: 1 }
"#;
    assert_eq!(
        paths(yaml),
        [
            "risk",
            "triggers.deploy",
            "triggers.push.extra",
            "budgets.x",
            "timeout.y",
            "receipts.z",
            "merge.w",
            "evidence.v",
            "checks.c.u",
            "checks.c.partition.t",
            "checks.c.budget.files",
        ]
    );
    let message = errs("schema_version: 1\nrisk: high\n")[0].to_string();
    assert!(message.starts_with("risk: unknown key; the keys allowed here are schema_version"));
}

#[test]
fn reports_every_problem_not_just_the_first() {
    let yaml = r#"
schema_version: 3
requires: "^1"
on_crash: explode
timeout: { factor: 0 }
receipts: { keep: -1 }
budgets: { on_exceed: never, regression: { window: 0, threshold: lots } }
merge: { mode: yolo, method: "" }
evidence: { accept: none }
triggers: { commit: { scope: everything, budget: { duration: 3 } }, ci: { budget: {} } }
"#;
    assert_eq!(
        paths(yaml),
        [
            "schema_version",
            "requires",
            "triggers.commit.scope",
            "triggers.commit.budget.duration",
            "triggers.ci.budget.duration",
            "budgets.on_exceed",
            "budgets.regression.window",
            "budgets.regression.threshold",
            "on_crash",
            "timeout.factor",
            "receipts.keep",
            "merge.mode",
            "merge.method",
            "evidence.accept",
        ]
    );
}

#[test]
fn wrong_shapes_are_type_errors() {
    for (yaml, path) in [
        ("schema_version: 1\ntriggers: []\n", "triggers"),
        ("schema_version: 1\nchecks: [a]\n", "checks"),
        ("schema_version: 1\nchecks: { c: run }\n", "checks.c"),
        ("schema_version: 1\nbudgets: 3\n", "budgets"),
        (
            "schema_version: 1\ntimeout: { factor: fast }\n",
            "timeout.factor",
        ),
    ] {
        let found = errs(yaml);
        assert_eq!(found.len(), 1, "{yaml}");
        assert!(
            matches!(&found[0], ConfigError::Type { path: p, .. } if p == path),
            "{yaml}"
        );
    }
}

#[test]
fn a_fractional_timeout_factor_is_kept() {
    assert_eq!(
        ok("schema_version: 1\ntimeout: { factor: 1.5 }\n").timeout_factor,
        1.5
    );
}

#[test]
fn a_check_needs_exactly_one_form() {
    assert_eq!(paths(&with_check("    on: [ci]\n")), ["checks.c.run"]);
    assert_eq!(
        paths(&with_check("    run: x\n    native: urzua\n")),
        ["checks.c"]
    );
    assert_eq!(
        paths(&with_check("    native: urzua\n    parser: oxlint\n")),
        ["checks.c.parser"]
    );
    assert_eq!(paths(&with_check("    native: ''\n")), ["checks.c.native"]);
}

#[test]
fn a_run_check_without_a_parser_uses_exit_code() {
    let c = ok(&with_check("    run: cargo test\n"));
    match &c.checks[0].form {
        CheckForm::Run {
            run,
            command,
            parser,
        } => {
            assert_eq!(run, "cargo test");
            assert_eq!(parser, DEFAULT_PARSER);
            assert!(!command.has_placeholder());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bad_run_string_is_reported_at_run() {
    let found = errs(&with_check("    run: \"oxlint --files={files}\"\n"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path(), "checks.c.run");
    assert!(found[0].to_string().contains("argument on its own"));
    assert_eq!(
        paths(&with_check("    run: \"sh -c 'a\"\n")),
        ["checks.c.run"]
    );
    assert_eq!(paths(&with_check("    run: \"  \"\n")), ["checks.c.run"]);
}

#[test]
fn check_fields_parse() {
    let c = ok(&with_check(
        "    run: x {files}\n    kind: types\n    on_exceed: fail\n    advisory: true\n    \
         budget: { duration: 500ms }\n    on: [ci, commit]\n",
    ));
    let check = &c.checks[0];
    assert_eq!(check.kind, Some(CheckKind::Types));
    assert_eq!(check.on_exceed, Some(OnExceed::Fail));
    assert!(check.advisory);
    assert_eq!(
        check.budget.map(|b| b.duration),
        Some(Duration::from_millis(500))
    );
    assert_eq!(check.on, [Trigger::Commit, Trigger::Ci]);
    assert!(check.globs.is_empty());
}

#[test]
fn check_field_errors_are_all_reported() {
    let yaml = with_check(
        "    run: x\n    kind: style\n    on_exceed: maybe\n    advisory: yes\n    \
         globs: [a, 1, '']\n    exclude: b\n    via: 3\n    budget: { duration: 1.5s }\n",
    );
    assert_eq!(
        paths(&yaml),
        [
            "checks.c.globs[1]",
            "checks.c.globs[2]",
            "checks.c.exclude",
            "checks.c.via",
            "checks.c.kind",
            "checks.c.budget.duration",
            "checks.c.on_exceed",
            "checks.c.advisory",
        ]
    );
}

#[test]
fn on_must_be_a_non_empty_list_of_distinct_triggers() {
    assert_eq!(
        paths(&with_check("    run: x\n    on: []\n")),
        ["checks.c.on"]
    );
    assert_eq!(
        paths(&with_check("    run: x\n    on: ci\n")),
        ["checks.c.on"]
    );
    assert_eq!(
        paths(&with_check("    run: x\n    on: [ci, deploy, ci]\n")),
        ["checks.c.on[1]", "checks.c.on[2]"]
    );
}

#[test]
fn partition_needs_by_dir_and_roots() {
    let p = |body: &str| paths(&with_check(&format!("    run: x\n    partition: {body}\n")));
    assert_eq!(p("{ roots: [a] }"), ["checks.c.partition.by"]);
    assert_eq!(p("{ by: package, roots: [a] }"), ["checks.c.partition.by"]);
    assert_eq!(p("{ by: dir }"), ["checks.c.partition.roots"]);
    assert_eq!(p("{ by: dir, roots: [] }"), ["checks.c.partition.roots"]);
    assert_eq!(p("dir"), ["checks.c.partition"]);
}

#[test]
fn errors_name_their_path_in_the_message() {
    for e in errs(&with_check("    run: x\n    on: []\n    kind: z\n")) {
        assert!(e.to_string().starts_with(e.path()), "{e}");
    }
    let all = Config::parse(&with_check("    on: []\n")).unwrap_err();
    assert_eq!(all.to_string().lines().count(), 2);
}

fn check_with(budget: Option<u64>, on: Vec<Trigger>, on_exceed: Option<OnExceed>) -> Check {
    Check {
        name: "c".into(),
        form: CheckForm::Native { tool: "t".into() },
        globs: Vec::new(),
        exclude: Vec::new(),
        partition: None,
        via: None,
        cwd: None,
        kind: None,
        budget: budget.map(|s| Budget {
            duration: Duration::from_secs(s),
        }),
        on_exceed,
        on,
        advisory: false,
    }
}

#[test]
fn effective_on_exceed_prefers_the_check() {
    let c = ok("schema_version: 1\nbudgets: { on_exceed: demote }\n");
    assert_eq!(
        c.on_exceed(&check_with(None, vec![], None)),
        OnExceed::Demote
    );
    assert_eq!(
        c.on_exceed(&check_with(None, vec![], Some(OnExceed::Fail))),
        OnExceed::Fail
    );
}

#[test]
fn timeout_uses_check_then_trigger_budget_then_ten_minutes() {
    let c = ok(
        "schema_version: 1\ntimeout: { factor: 3 }\ntriggers: { push: { budget: { duration: 60s } } }\n",
    );
    let with_budget = check_with(Some(2), vec![], None);
    let without = check_with(None, vec![], None);
    assert_eq!(
        c.timeout(&with_budget, Trigger::Push),
        Duration::from_secs(6)
    );
    assert_eq!(c.timeout(&without, Trigger::Push), Duration::from_secs(180));
    assert_eq!(c.timeout(&without, Trigger::Commit), DEFAULT_TIMEOUT);
    assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(600));
}

#[test]
fn timeout_saturates_instead_of_overflowing() {
    let c = ok("schema_version: 1\ntimeout: { factor: 1.0e300 }\n");
    assert_eq!(
        c.timeout(&check_with(Some(1), vec![], None), Trigger::Ci),
        Duration::MAX
    );
}

#[test]
fn next_trigger_follows_the_checks_own_on_list() {
    let check = check_with(None, vec![Trigger::Commit, Trigger::Ci], None);
    assert_eq!(check.next_trigger(Trigger::Commit), Some(Trigger::Ci));
    assert_eq!(check.next_trigger(Trigger::Ci), None);
    assert_eq!(check.next_trigger(Trigger::Push), Some(Trigger::Ci));
}

#[test]
fn requires_is_checked_against_a_version() {
    let c = ok("schema_version: 1\nrequires: '>=0.1.0'\n");
    let req = c.requires.unwrap();
    assert_eq!(satisfies(&req, "0.1.0"), Ok(true));
    assert_eq!(satisfies(&req, "0.0.9"), Ok(false));
}

#[test]
fn trigger_worktree_is_current_or_merged_on_any_trigger() {
    let c = ok("schema_version: 1\ntriggers: { ci: { worktree: merged } }\n");
    assert_eq!(c.trigger(Trigger::Ci).worktree, Worktree::Merged);
    assert_eq!(c.trigger(Trigger::Ci).scope, ScopeKind::All);
    assert_eq!(c.trigger(Trigger::Merge).worktree, Worktree::Current);
    assert_eq!(
        paths("schema_version: 1\ntriggers: { merge: { worktree: main } }\n"),
        ["triggers.merge.worktree"]
    );
}

#[test]
fn a_builtin_check_parses() {
    let c = ok(&with_check(
        "    builtin: clean-tree\n    kind: other\n    on: [ci]\n",
    ));
    assert_eq!(
        c.checks[0].form,
        CheckForm::Builtin {
            builtin: Builtin::CleanTree
        }
    );
    assert_eq!(c.checks[0].kind, Some(CheckKind::Other));
}

#[test]
fn an_unknown_builtin_lists_the_known_ones() {
    let found = errs(&with_check("    builtin: tidy\n"));
    assert_eq!(found.len(), 1);
    assert!(matches!(&found[0], ConfigError::Invalid { path, .. } if path == "checks.c.builtin"));
    assert!(
        found[0]
            .to_string()
            .contains("the known builtins are clean-tree")
    );
}

#[test]
fn a_builtin_check_rejects_input_and_parser_keys() {
    let yaml = with_check(
        "    builtin: clean-tree\n    parser: x\n    globs: [a]\n    exclude: [b]\n    \
         partition: { by: dir, roots: [p] }\n    via: turbo\n    cwd: x\n",
    );
    assert_eq!(
        paths(&yaml),
        [
            "checks.c.parser",
            "checks.c.globs",
            "checks.c.partition",
            "checks.c.via",
            "checks.c.exclude",
            "checks.c.cwd",
        ]
    );
}

#[test]
fn only_one_form_may_be_declared() {
    let found = errs(&with_check("    run: x\n    builtin: clean-tree\n"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path(), "checks.c");
    assert!(found[0].to_string().contains("not `run` and `builtin`"));
    assert_eq!(
        paths(&with_check(
            "    run: x\n    native: u\n    builtin: clean-tree\n"
        )),
        ["checks.c"]
    );
}

#[test]
fn on_is_stored_in_trigger_order_and_demotes_only_later() {
    let c = ok(&with_check("    run: x\n    on: [merge, ci, commit]\n"));
    let check = &c.checks[0];
    assert_eq!(check.on, [Trigger::Commit, Trigger::Ci, Trigger::Merge]);
    assert_eq!(check.next_trigger(Trigger::Commit), Some(Trigger::Ci));
    assert_eq!(check.next_trigger(Trigger::Ci), Some(Trigger::Merge));
    assert_eq!(check.next_trigger(Trigger::Push), Some(Trigger::Ci));
    assert_eq!(check.next_trigger(Trigger::Merge), None);
    let reversed = ok(&with_check("    run: x\n    on: [ci, commit]\n"));
    assert_eq!(reversed.checks[0].next_trigger(Trigger::Ci), None);
}

#[test]
fn a_zero_duration_is_rejected_in_a_config() {
    let found = errs(&with_check("    run: x\n    budget: { duration: 0s }\n"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path(), "checks.c.budget.duration");
    assert!(found[0].to_string().contains("greater than zero"));
    assert_eq!(
        paths("schema_version: 1\ntriggers: { ci: { budget: { duration: 0ms } } }\n"),
        ["triggers.ci.budget.duration"]
    );
}

#[test]
fn yaml_tags_are_rejected_at_their_path() {
    let found = errs(&with_check("    run: !foo x\n"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path(), "checks.c.run");
    assert!(found[0].to_string().contains("!foo"), "{}", found[0]);
    assert_eq!(
        paths("schema_version: 1\nchecks:\n  c:\n    run: x\n    on: [ci, !t push]\n"),
        ["checks.c.on[1]"]
    );
    assert_eq!(paths("schema_version: !v 1\n"), ["schema_version"]);
}

#[test]
fn anchors_and_aliases_are_accepted() {
    let c = ok("schema_version: 1\nchecks:\n  a: &base\n    run: x\n    on: [ci]\n  b: *base\n");
    assert_eq!(c.checks.len(), 2);
    assert_eq!(c.checks[1].on, [Trigger::Ci]);
}

#[test]
fn a_check_name_that_reads_as_a_flag_is_rejected() {
    for name in ["--all", "-x", "a b", "", "lint!"] {
        let yaml = format!("schema_version: 1\nchecks:\n  \"{name}\":\n    run: x\n");
        let errors = Config::parse(&yaml).unwrap_err();
        assert!(
            errors
                .0
                .iter()
                .any(|e| e.to_string().contains("check name")),
            "{name:?}: {errors}"
        );
    }
    let ok = "schema_version: 1\nchecks:\n  clean-tree_2.x:\n    run: x\n";
    assert!(Config::parse(ok).is_ok());
}

#[test]
fn cwd_is_a_normalised_repository_relative_directory() {
    let c = Config::parse(&with_check("    run: x\n    cwd: ./tools//lint/\n")).unwrap();
    assert_eq!(c.check("c").unwrap().cwd.as_deref(), Some("tools/lint"));
    let c = Config::parse(&with_check("    run: x\n    cwd: .\n")).unwrap();
    assert_eq!(c.check("c").unwrap().cwd, None);
    for bad in ["/abs", "a/../b", "..", "a\\\\b"] {
        let yaml = with_check(&format!("    run: x\n    cwd: '{bad}'\n"));
        assert_eq!(paths(&yaml), ["checks.c.cwd"], "{bad}");
    }
}

#[test]
fn cwd_and_partition_do_not_combine() {
    let yaml = with_check("    run: x\n    cwd: a\n    partition: { by: dir, roots: [p] }\n");
    assert_eq!(paths(&yaml), ["checks.c.cwd"]);
}

#[test]
fn via_names_turbo_only() {
    let yaml = with_check("    run: nx run {affected}\n    via: nx\n");
    assert_eq!(paths(&yaml), ["checks.c.via"]);
    assert!(
        errs(&yaml)[0]
            .to_string()
            .contains("the only one is `turbo`")
    );
}

#[test]
fn affected_needs_a_task_runner() {
    let yaml = with_check("    run: tool {affected}\n");
    assert_eq!(paths(&yaml), ["checks.c.run"]);
    assert!(errs(&yaml)[0].to_string().contains("add `via: turbo`"));
    assert!(Config::parse(&with_check("    run: turbo t {affected}\n    via: turbo\n")).is_ok());
}

#[test]
fn the_check_name_garita_is_reserved() {
    let found = errs("schema_version: 1\nchecks:\n  garita:\n    run: x\n");
    assert_eq!(found.len(), 1);
    assert!(matches!(&found[0], ConfigError::Invalid { path, .. } if path == "checks.garita"));
    assert!(found[0].to_string().contains("reserved"));
}

#[test]
fn the_rfc1_example_requires_the_running_version() {
    let c = ok(RFC1_EXAMPLE);
    let requires = c.requires.as_ref().unwrap();
    assert_eq!(satisfies(requires, env!("CARGO_PKG_VERSION")), Ok(true));
}
