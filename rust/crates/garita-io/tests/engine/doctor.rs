use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use garita_core::report::{Report, RunStatus, Severity, Trigger};
use garita_io::doctor::{self, Request};
use garita_io::hooks;

use crate::support::{Repo, assert_contract, finding, git_in, run_rules};

/// A repository where everything doctor looks at is in order: a resolvable tool, both hooks,
/// receipts ignored, `garita` on PATH and a check at each hook's trigger.
fn healthy() -> Repo {
    let repo = Repo::new();
    repo.tool("garita", "exit 0");
    repo.tool("lint", "exit 0");
    repo.config(
        "schema_version: 1\nchecks:\n  lint:\n    run: lint {files}\n    on: [commit, push]\n",
    );
    assert_eq!(hooks::install(&repo.root(), false).exit_code(), 0);
    repo
}

fn path_of(repo: &Repo) -> OsString {
    repo.base().join("bin").into_os_string()
}

fn doctor_at(dir: &Path, path: Option<OsString>) -> Report {
    let v = doctor::doctor(&Request {
        dir: dir.to_path_buf(),
        version: "0.1.0".to_string(),
        path,
    });
    assert_contract(&v.report);
    v.report
}

fn doctor(repo: &Repo) -> Report {
    doctor_at(&repo.root(), Some(path_of(repo)))
}

trait Unhooked {
    fn commit_unhooked(&self, message: &str);
}

impl Unhooked for Repo {
    /// The installed hooks would call a `garita` this process cannot see.
    fn commit_unhooked(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--no-verify", "-m", message]);
    }
}

fn has(report: &Report, rule: &str) -> bool {
    run_rules(report).contains(&rule)
}

fn hook(repo: &Repo, name: &str) -> std::path::PathBuf {
    repo.root().join(".git/hooks").join(name)
}

#[test]
fn a_healthy_repository_passes_with_what_it_examined() {
    let repo = healthy();
    let report = doctor(&repo);
    assert_eq!(report.status, RunStatus::Pass, "{report:#?}");
    assert_eq!(report.exit_code(), 0);
    assert_eq!(report.examined.get("checks"), Some(&1));
    assert_eq!(report.examined.get("hooks"), Some(&2));
    assert!(report.next_action.is_none());
    assert!(
        report.findings.iter().all(|f| f.severity == Severity::Info),
        "{report:#?}"
    );
}

#[test]
fn outside_a_repository_doctor_is_not_run() {
    let repo = Repo::new();
    let report = doctor_at(&repo.base().join("bin"), None);
    assert_eq!(report.status, RunStatus::NotRun);
    assert_eq!(report.exit_code(), 2);
}

#[test]
fn a_missing_config_is_not_run_as_garita_run_reports_it() {
    let repo = Repo::new();
    let report = doctor(&repo);
    assert_eq!(report.exit_code(), 2);
    let run = repo.run_at(Trigger::Commit).report;
    assert_eq!(report.error, run.error);
}

#[test]
fn an_invalid_config_lists_every_error_and_is_not_run() {
    let repo = healthy();
    repo.config(
        "schema_version: 1\nchecks:\n  a:\n    run: 3\n  b:\n    on: [lunch]\n    run: x\n",
    );
    let report = doctor(&repo);
    assert_eq!(report.exit_code(), 2);
    let error = report.error.unwrap();
    assert!(
        error.contains("checks.a.run") && error.contains("checks.b.on"),
        "{error}"
    );
}

#[test]
fn an_unmet_requires_is_not_run() {
    let repo = healthy();
    repo.config("schema_version: 1\nrequires: \">=9.0.0\"\nchecks: {}\n");
    let report = doctor(&repo);
    assert_eq!(report.exit_code(), 2);
    assert!(report.error.unwrap().contains("requires"));
}

#[test]
fn a_missing_hook_is_a_warning_whose_next_action_installs_it() {
    let repo = healthy();
    fs::remove_file(hook(&repo, "pre-push")).unwrap();
    let report = doctor(&repo);
    assert_eq!(report.status, RunStatus::Warn);
    assert_eq!(
        run_rules(&report)
            .iter()
            .filter(|r| **r == "doctor/hook-missing")
            .count(),
        1
    );
    assert_eq!(report.next_action.as_deref(), Some("garita hooks install"));
    assert!(report.next_action_reason.unwrap().contains("pre-push"));
    assert!(!has(&doctor(&healthy()), "doctor/hook-missing"));
}

#[test]
fn a_foreign_hook_is_a_warning_unless_it_runs_garita() {
    let repo = healthy();
    fs::write(hook(&repo, "pre-commit"), "#!/bin/sh\nnpm test\n").unwrap();
    let report = doctor(&repo);
    assert_eq!(
        finding(&report, "doctor/hook-foreign").severity,
        Severity::Warning
    );
    assert!(report.next_action.is_none());
    fs::write(
        hook(&repo, "pre-commit"),
        "#!/bin/sh\nnpm test\ngarita run --trigger commit\n",
    )
    .unwrap();
    let report = doctor(&repo);
    assert!(!has(&report, "doctor/hook-foreign"));
    assert_eq!(
        finding(&report, "doctor/hook-calls-garita").severity,
        Severity::Info
    );
    assert_eq!(report.status, RunStatus::Pass);
}

#[test]
fn garitas_hook_with_a_line_appended_is_modified_not_garitas() {
    let repo = healthy();
    let path = hook(&repo, "pre-commit");
    let edited = format!(
        "{}echo extra\n",
        hooks::script("pre-commit", Trigger::Commit)
    );
    fs::write(&path, edited).unwrap();
    let report = doctor(&repo);
    assert_eq!(
        finding(&report, "doctor/hook-modified").severity,
        Severity::Warning
    );
    assert!(!has(&report, "doctor/hook-foreign"));
    assert!(!has(&doctor(&healthy()), "doctor/hook-modified"));
}

#[test]
fn a_format_1_hook_is_outdated_and_its_next_action_installs() {
    let repo = healthy();
    let path = hook(&repo, "pre-push");
    let text = fs::read_to_string(&path).unwrap();
    let old = text
        .replace("format 2:", "format 1:")
        .replace(
            &format!(
                "printf '%s\\n' '{}'",
                garita_io::hooks::missing_garita_report("pre-push", Trigger::Push)
            ),
            "echo \"garita: the pre-push hook needs garita, which is not on PATH; this push is blocked\" >&2",
        );
    fs::write(&path, old).unwrap();
    let report = doctor(&repo);
    assert!(has(&report, "doctor/hook-outdated"));
    assert_eq!(report.next_action.as_deref(), Some("garita hooks install"));
    assert_eq!(
        report.next_action_argv,
        Some(vec![
            "garita".to_string(),
            "hooks".to_string(),
            "install".to_string()
        ])
    );
}

#[test]
fn a_current_hook_is_not_outdated() {
    assert!(!has(&doctor(&healthy()), "doctor/hook-outdated"));
}

#[test]
fn only_a_garita_run_command_at_this_hooks_trigger_counts_as_calling_garita() {
    let repo = healthy();
    let cases = [
        (
            "pre-commit",
            "npm test\nexec garita run --trigger commit\n",
            true,
        ),
        ("pre-commit", "garita run\n", true),
        (
            "pre-commit",
            "  garita run --trigger=commit --files a\n",
            true,
        ),
        (
            "pre-commit",
            "echo \"garita run is disabled\"; exit 0\n",
            false,
        ),
        ("pre-commit", "garita run --trigger push\n", false),
        ("pre-commit", "garita run --trigger commit || true\n", false),
        ("pre-commit", "garita run --trigger commit &\n", false),
        ("pre-commit", "garita run; exit 0\n", false),
        (
            "pre-commit",
            "garita run --trigger commit \\\n  || true\n",
            false,
        ),
        (
            "pre-commit",
            "exec garita run --trigger commit 2>&1\n",
            true,
        ),
        ("pre-commit", "garita run --trigger commit >&2\n", true),
        ("pre-commit", "garita run --trigger commit 2>&1 &\n", false),
        ("pre-commit", "garita runner\n", false),
        ("pre-commit", "# garita run --trigger commit\n", false),
        ("pre-push", "garita run\n", false),
        ("pre-push", "exec garita run --trigger push \"$@\"\n", true),
    ];
    for (name, body, calls) in cases {
        fs::write(hook(&repo, name), format!("#!/bin/sh\n{body}")).unwrap();
        let report = doctor(&repo);
        assert_eq!(
            has(&report, "doctor/hook-calls-garita"),
            calls,
            "{name}: {body}"
        );
        assert_eq!(
            has(&report, "doctor/hook-foreign"),
            !calls,
            "{name}: {body}"
        );
        hooks::install(&repo.root(), true);
    }
}

#[test]
fn a_hook_that_is_not_executable_is_an_error() {
    let repo = healthy();
    fs::set_permissions(hook(&repo, "pre-push"), fs::Permissions::from_mode(0o644)).unwrap();
    let report = doctor(&repo);
    assert_eq!(report.exit_code(), 1);
    assert_eq!(
        finding(&report, "doctor/hook-not-executable").severity,
        Severity::Error
    );
    assert_eq!(report.next_action.as_deref(), Some("garita hooks install"));
    assert!(!has(&doctor(&healthy()), "doctor/hook-not-executable"));
}

#[test]
fn garita_missing_from_path_is_an_error() {
    let repo = healthy();
    fs::remove_file(repo.base().join("bin/garita")).unwrap();
    let report = doctor(&repo);
    assert_eq!(
        finding(&report, "doctor/garita-not-on-path").severity,
        Severity::Error
    );
    assert_eq!(report.exit_code(), 1);
    let none = doctor_at(&repo.root(), None);
    assert!(has(&none, "doctor/garita-not-on-path"));
}

#[test]
fn a_tool_that_cannot_be_found_is_an_error_naming_the_check_and_program() {
    let repo = healthy();
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: nosuch-lint {files}\n    on: [commit, push]\n");
    let f = finding(&doctor(&repo), "doctor/tool-missing").clone();
    assert_eq!(f.severity, Severity::Error);
    assert!(
        f.message.contains("lint") && f.message.contains("nosuch-lint"),
        "{}",
        f.message
    );
    fs::set_permissions(
        repo.base().join("bin/lint"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    repo.config(
        "schema_version: 1\nchecks:\n  lint:\n    run: lint {files}\n    on: [commit, push]\n",
    );
    let report = doctor(&repo);
    assert!(!has(&report, "doctor/tool-missing"));
    assert_eq!(
        finding(&report, "doctor/tool-not-executable").severity,
        Severity::Error
    );
}

#[test]
fn a_tool_with_a_slash_resolves_against_the_checks_cwd() {
    let repo = healthy();
    repo.write("pkg/bin/lint", "#!/bin/sh\nexit 0\n");
    fs::set_permissions(
        repo.root().join("pkg/bin/lint"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: ./bin/lint {files}\n    cwd: pkg\n    on: [commit, push]\n");
    assert!(!has(&doctor(&repo), "doctor/tool-missing"));
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: ./bin/lint {files}\n    on: [commit, push]\n");
    assert!(has(&doctor(&repo), "doctor/tool-missing"));
}

#[test]
fn an_unknown_parser_is_an_error() {
    let repo = healthy();
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: lint {files}\n    parser: nope\n    on: [commit, push]\n");
    assert_eq!(
        finding(&doctor(&repo), "doctor/unknown-parser").severity,
        Severity::Error
    );
    assert!(!has(&doctor(&healthy()), "doctor/unknown-parser"));
}

#[test]
fn native_and_builtin_checks_warn_that_this_version_does_not_run_them() {
    let repo = healthy();
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: lint {files}\n    on: [commit, push]\n  records:\n    native: urzua\n    on: [ci]\n  clean:\n    builtin: clean-tree\n    on: [ci]\n");
    let report = doctor(&repo);
    let count = run_rules(&report)
        .iter()
        .filter(|r| **r == "doctor/form-unsupported")
        .count();
    assert_eq!(count, 2);
    assert_eq!(report.status, RunStatus::Warn);
    assert!(!has(&doctor(&healthy()), "doctor/form-unsupported"));
}

#[test]
fn via_turbo_adds_nothing() {
    let repo = healthy();
    repo.tool("turbo", "exit 0");
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: turbo run lint {affected}\n    via: turbo\n    on: [commit, push]\n");
    let report = doctor(&repo);
    assert_eq!(report.status, RunStatus::Pass, "{report:#?}");
}

#[test]
fn a_missing_cwd_or_partition_root_is_an_error() {
    let repo = healthy();
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: lint {files}\n    cwd: gone\n    on: [commit, push]\n");
    assert_eq!(
        finding(&doctor(&repo), "doctor/cwd-missing").severity,
        Severity::Error
    );
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: lint {files}\n    partition: { by: dir, roots: ['packages/*'] }\n    on: [commit, push]\n");
    assert_eq!(
        finding(&doctor(&repo), "doctor/partition-root-missing").severity,
        Severity::Error
    );
    repo.write("packages/app/a.ts", "x\n");
    repo.stage(&["packages/app/a.ts"]);
    let report = doctor(&repo);
    assert!(!has(&report, "doctor/partition-root-missing"));
    assert!(!has(&report, "doctor/cwd-missing"));
}

#[test]
fn receipts_not_ignored_is_a_warning_with_the_line_to_add() {
    let repo = healthy();
    repo.write(".gitignore", ".turbo/\n");
    let report = doctor(&repo);
    let f = finding(&report, "doctor/receipts-not-ignored");
    assert_eq!(f.severity, Severity::Warning);
    assert!(f.message.contains("`.garita/receipts/`"), "{}", f.message);
    assert_eq!(report.next_action, None);
    assert_eq!(report.next_action_argv, None);
    assert!(!has(&doctor(&healthy()), "doctor/receipts-not-ignored"));
}

#[test]
fn a_shallow_clone_is_info() {
    let origin = healthy();
    origin.commit_unhooked("one");
    origin.write("b.ts", "b\n");
    origin.commit_unhooked("two");
    let url = format!("file://{}", origin.root().display());
    let clone = origin.base().join("clone");
    git_in(
        origin.base(),
        &["clone", "-q", "--depth", "1", &url, clone.to_str().unwrap()],
    );
    let repo = origin.adopt("clone");
    repo.tool("garita", "exit 0");
    repo.tool("lint", "exit 0");
    let report = doctor(&repo);
    assert_eq!(finding(&report, "doctor/shallow").severity, Severity::Info);
    assert!(!has(&doctor(&healthy()), "doctor/shallow"));
}

#[test]
fn an_unresolvable_base_is_info_naming_the_candidates_tried() {
    let repo = healthy();
    repo.commit_unhooked("one");
    let report = doctor(&repo);
    let f = finding(&report, "doctor/base-unresolved");
    assert_eq!(f.severity, Severity::Info);
    assert!(
        f.message.contains("push") && f.message.contains("origin/main"),
        "{}",
        f.message
    );
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.write("c.ts", "c\n");
    repo.commit_unhooked("two");
    assert!(!has(&doctor(&repo), "doctor/base-unresolved"));
}

#[test]
fn a_trigger_with_no_check_is_info() {
    let repo = healthy();
    repo.config("schema_version: 1\nchecks:\n  lint:\n    run: lint {files}\n    on: [commit]\n");
    let f = finding(&doctor(&repo), "doctor/trigger-without-checks").clone();
    assert!(f.message.contains("push"), "{}", f.message);
    assert!(!has(&doctor(&healthy()), "doctor/trigger-without-checks"));
}

fn snapshot(root: &Path) -> (String, BTreeMap<String, Vec<u8>>) {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                out.insert(format!("{rel}/"), Vec::new());
                walk(&path, root, out);
            } else {
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    // `--no-optional-locks` keeps this status from refreshing the index it is about to compare.
    let status = git_in(
        root,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=all",
        ],
    );
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    (status, files)
}

fn differing(a: &BTreeMap<String, Vec<u8>>, b: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    a.keys()
        .chain(b.keys())
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[test]
fn doctor_leaves_the_repository_byte_identical() {
    let repo = healthy();
    repo.commit_unhooked("one");
    repo.write("a.ts", "changed\n");
    repo.write("new.ts", "new\n");
    let before = snapshot(&repo.root());
    doctor(&repo);
    let after = snapshot(&repo.root());
    assert_eq!(differing(&before.1, &after.1), Vec::<String>::new());
    assert_eq!(after.0, before.0);
    assert!(repo.receipts().is_empty());
}

#[test]
fn whenever_run_at_commit_is_not_run_doctor_does_not_pass() {
    let defects = [
        "    native: urzua\n",
        "    builtin: clean-tree\n",
        "    run: BIN/nosuch-lint {files}\n",
        "    run: BIN/unexecutable {files}\n",
        "    run: BIN/lint {files}\n    parser: nope\n",
        "    run: BIN/lint {files}\n    cwd: gone\n",
        "    run: BIN/lint {files}\n    partition: { by: dir, roots: ['packages/*'] }\n",
        "    run: '{files}'\n",
    ];
    let mut not_run = 0;
    for body in defects {
        let repo = healthy();
        let bin = repo.base().join("bin/unexecutable");
        fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
        let body = body.replace("BIN", &repo.base().join("bin").to_string_lossy());
        repo.config(&format!(
            "schema_version: 1\nchecks:\n  c:\n{body}    on: [commit, push]\n"
        ));
        repo.write("a.ts", "x\n");
        repo.stage(&["a.ts"]);
        let run = repo.run_at(Trigger::Commit).report;
        let report = doctor(&repo);
        if run.status == RunStatus::NotRun {
            not_run += 1;
            assert_ne!(report.exit_code(), 0, "{body}: {report:#?}");
        }
        assert_ne!(report.exit_code(), 0, "{body}: doctor passed: {report:#?}");
    }
    assert!(not_run >= 6, "{not_run}");
}

#[test]
fn a_defect_in_a_check_that_runs_only_in_ci_is_a_warning() {
    let repo = healthy();
    repo.config(
        "schema_version: 1\nchecks:\n  lint:\n    run: lint {files}\n    on: [commit, push]\n  records:\n    native: urzua\n    on: [ci]\n  ci-only:\n    run: nosuch-lint {files}\n    on: [ci, merge]\n",
    );
    let report = doctor(&repo);
    assert_eq!(report.exit_code(), 0, "{report:#?}");
    assert_eq!(
        finding(&report, "doctor/form-unsupported").severity,
        Severity::Warning
    );
    assert_eq!(
        finding(&report, "doctor/tool-missing").severity,
        Severity::Warning
    );
}
