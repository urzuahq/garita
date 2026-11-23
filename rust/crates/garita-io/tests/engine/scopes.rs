use garita_core::report::{CheckStatus, ScopeKind, Severity, Trigger};

use crate::support::{Repo, check, finding, git_in, rules};

fn recorder_config(repo: &Repo, extra: &str) -> String {
    let tool = repo.recorder("tool");
    format!("schema_version: 1\nchecks:\n  tool:\n    run: {tool} {{files}}\n{extra}")
}

fn files_seen(repo: &Repo) -> Vec<String> {
    let log = repo.log("tool");
    let mut files: Vec<String> = log
        .iter()
        .flat_map(|line| {
            line.split_once('|')
                .unwrap()
                .1
                .split(' ')
                .map(str::to_string)
        })
        .filter(|w| !w.is_empty())
        .collect();
    files.sort();
    files
}

#[test]
fn staged_scope_sees_only_staged_files() {
    let repo = Repo::new();
    repo.config(&recorder_config(&repo, ""));
    repo.write("a.ts", "a\n");
    repo.write("b.ts", "b\n");
    repo.stage(&["a.ts"]);
    let out = repo.run();
    assert_eq!(files_seen(&repo), ["a.ts"]);
    let scope = out.report.scope.as_ref().unwrap();
    assert_eq!(scope.kind, ScopeKind::Staged);
    assert_eq!(scope.files.as_deref(), Some(&["a.ts".to_string()][..]));
    assert_eq!(out.report.commit, None);
}

#[test]
fn changed_scope_sees_the_files_changed_against_the_base() {
    let repo = Repo::new();
    repo.config(&recorder_config(&repo, ""));
    repo.write("old.ts", "o\n");
    repo.commit_all("base");
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.write("new.ts", "n\n");
    repo.commit_all("change");
    let mut request = repo.request(Trigger::Push);
    request.base = Some("main".into());
    let out = repo.run_request(&request);
    assert_eq!(files_seen(&repo), ["new.ts"]);
    assert_eq!(out.report.scope.as_ref().unwrap().kind, ScopeKind::Changed);
    assert!(out.report.commit.is_some());
    assert!(!rules(&out.report, "garita").contains(&"scope-widened"));
}

#[test]
fn changed_scope_finds_a_default_base_without_being_told() {
    let repo = Repo::new();
    repo.config(&recorder_config(&repo, ""));
    repo.commit_all("base");
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.write("new.ts", "n\n");
    repo.commit_all("change");
    repo.run_at(Trigger::Push);
    assert_eq!(files_seen(&repo), ["new.ts"]);
}

#[test]
fn a_missing_base_widens_to_all_with_scope_widened() {
    let repo = Repo::with_branch("trunk");
    repo.config(&recorder_config(&repo, "    globs: ['*.ts']\n"));
    repo.write("a.ts", "a\n");
    repo.commit_all("one");
    let out = repo.run_at(Trigger::Push);
    let f = finding(&out.report, "scope-widened");
    assert_eq!((f.severity, f.check.as_str()), (Severity::Info, "garita"));
    assert!(f.message.contains("no base ref"), "{}", f.message);
    assert_eq!(out.report.scope.as_ref().unwrap().kind, ScopeKind::All);
    assert_eq!(files_seen(&repo), ["a.ts"]);
}

#[test]
fn a_shallow_clone_widens_to_all_with_scope_widened() {
    let origin = Repo::new();
    origin.write("a.ts", "a\n");
    origin.commit_all("one");
    origin.write("b.ts", "b\n");
    origin.commit_all("two");
    origin.git(&["switch", "-q", "-c", "feature", "HEAD~1"]);
    origin.write("c.ts", "c\n");
    origin.commit_all("three");
    origin.git(&["switch", "-q", "main"]);
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
    repo.config(&recorder_config(&repo, "    globs: ['*.ts']\n"));
    let out = repo.run_at(Trigger::Push);
    let f = finding(&out.report, "scope-widened");
    assert!(f.message.contains("shallow"), "{}", f.message);
    assert_eq!(files_seen(&repo), ["a.ts", "c.ts"]);
}

#[test]
fn files_replace_the_triggers_scope() {
    let repo = Repo::new();
    repo.config(&recorder_config(&repo, ""));
    repo.write("a.ts", "a\n");
    repo.write("b.ts", "b\n");
    repo.stage(&["a.ts"]);
    let mut request = repo.request(Trigger::Commit);
    request.files = Some(vec!["./b.ts".into()]);
    let out = repo.run_request(&request);
    assert_eq!(files_seen(&repo), ["b.ts"]);
    assert_eq!(
        out.report.scope.as_ref().unwrap().files.as_deref(),
        Some(&["b.ts".to_string()][..])
    );
}

#[test]
fn files_outside_the_repository_are_not_run() {
    let repo = Repo::new();
    repo.config(&recorder_config(&repo, ""));
    let mut request = repo.request(Trigger::Commit);
    request.files = Some(vec!["../x.ts".into()]);
    let out = repo.run_request(&request);
    assert_eq!(out.exit_code(), 2);
    assert!(repo.log("tool").is_empty());
}

#[test]
fn a_partly_staged_file_is_a_warning_once() {
    let repo = Repo::new();
    repo.config(&recorder_config(&repo, ""));
    repo.write("a.ts", "a\n");
    repo.stage(&["a.ts"]);
    repo.write("a.ts", "a changed\n");
    let out = repo.run();
    let found: Vec<_> = out
        .report
        .findings
        .iter()
        .filter(|f| f.rule == "partly-staged")
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].check, "garita");
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].file.as_deref(), Some("a.ts"));
    assert_eq!(out.exit_code(), 0);
}

#[test]
fn globs_and_exclude_choose_a_checks_files() {
    let repo = Repo::new();
    repo.config(&recorder_config(
        &repo,
        "    globs: ['**/*.{ts,tsx}']\n    exclude: ['gen/**']\n",
    ));
    for f in ["a.ts", "src/b.tsx", "c.js", "gen/d.ts"] {
        repo.write(f, "x\n");
    }
    repo.stage(&["."]);
    repo.run();
    assert_eq!(files_seen(&repo), ["a.ts", "src/b.tsx"]);
}

fn partition_repo() -> Repo {
    let repo = Repo::new();
    let log = repo.log_path("lint");
    let dirty = crate::support::replay_body("oxlint@1.94.0/one-error-in-package-json");
    let clean = crate::support::replay_body("oxlint@1.94.0/clean-json");
    let tool = repo.tool(
        "oxlint",
        &format!(
            "printf '%s|%s\\n' \"$PWD\" \"$*\" >> '{}'\ncase \"$PWD\" in\n*/packages/app) {dirty};;\n*) {clean};;\nesac",
            log.display()
        ),
    );
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  lint:\n    run: {tool} {{files}}\n    parser: oxlint\n    \
         partition: {{ by: dir, roots: ['packages/*'] }}\n    exclude: ['packages/docs/**']\n"
    ));
    let files = [
        "packages/app/src/lint-error.ts",
        "packages/lib/src/b.ts",
        "packages/docs/c.ts",
        "tools/d.ts",
    ];
    for f in files {
        repo.write(f, "x\n");
    }
    repo.stage(&files);
    repo
}

#[test]
fn partition_runs_once_per_root_with_paths_rebased_and_findings_mapped_back() {
    let repo = partition_repo();
    let out = repo.run();
    let root = repo.root();
    let mut log = repo.log("lint");
    log.sort();
    assert_eq!(
        log,
        [
            format!(
                "{}/packages/app|src/lint-error.ts --format=json",
                root.display()
            ),
            format!("{}/packages/lib|src/b.ts --format=json", root.display()),
        ]
    );
    let f = out
        .report
        .findings
        .iter()
        .find(|f| f.severity == Severity::Error)
        .unwrap();
    assert_eq!(f.file.as_deref(), Some("packages/app/src/lint-error.ts"));
    assert_eq!(f.line.map(|l| l.get()), Some(2));
    let outside = finding(&out.report, "outside-root");
    assert_eq!(outside.severity, Severity::Info);
    assert!(outside.message.contains("1 file"), "{}", outside.message);
    assert_eq!(check(&out.report, "lint").status, CheckStatus::Fail);
    let receipt = repo.receipt_of(&out.report);
    assert!(
        receipt.checks[0].command.contains("(cd packages/app && "),
        "{}",
        receipt.checks[0].command
    );
    assert!(receipt.checks[0].command.contains("; (cd packages/lib && "));
}

#[test]
fn cwd_runs_the_check_inside_that_directory() {
    let repo = Repo::new();
    let tool = repo.recorder("tool");
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  tool:\n    run: {tool} {{files}}\n    cwd: web\n"
    ));
    repo.write("web/a.ts", "a\n");
    repo.write("b.ts", "b\n");
    repo.stage(&["web/a.ts", "b.ts"]);
    let out = repo.run();
    assert_eq!(
        repo.log("tool"),
        [format!("{}/web|a.ts", repo.root().display())]
    );
    assert_eq!(
        finding(&out.report, "outside-root").severity,
        Severity::Info
    );
}

#[test]
fn a_missing_cwd_is_not_run() {
    let repo = Repo::new();
    let tool = repo.recorder("tool");
    repo.config(&format!(
        "schema_version: 1\nchecks:\n  tool:\n    run: {tool}\n    cwd: nowhere\n"
    ));
    let out = repo.run();
    assert_eq!(out.exit_code(), 2);
    assert!(out.report.error.as_deref().unwrap().contains("nowhere"));
}

#[test]
fn partly_staged_follows_the_staged_scope_not_the_trigger() {
    let repo = Repo::new();
    let config = recorder_config(&repo, "");
    repo.config(&config.replacen(
        "checks:",
        "triggers:\n  push: { scope: staged }\nchecks:",
        1,
    ));
    repo.write("a.ts", "a\n");
    repo.stage(&["a.ts"]);
    repo.write("a.ts", "a changed\n");
    let at_push = repo.run_at(Trigger::Push);
    assert_eq!(rules(&at_push.report, "garita"), ["partly-staged"]);
    let at_ci = repo.run_at(Trigger::Ci);
    assert!(!rules(&at_ci.report, "garita").contains(&"partly-staged"));
    let mut request = repo.request(Trigger::Commit);
    request.files = Some(vec!["a.ts".into()]);
    let explicit = repo.run_request(&request);
    assert!(rules(&explicit.report, "garita").is_empty());
}
