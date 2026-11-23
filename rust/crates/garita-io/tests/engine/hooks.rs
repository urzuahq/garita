use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use garita_core::report::{Report, RunStatus, Severity, Trigger};
use garita_io::hooks::{self, HOOKS, Kind};

use crate::support::{Repo, assert_contract, finding, git_in, rules};

const CONFIG: &str = "schema_version: 1\nchecks: {}\n";

fn install(dir: &Path, replace: bool) -> Report {
    let v = hooks::install(dir, replace);
    assert_contract(&v.report);
    v.report
}

fn uninstall(dir: &Path) -> Report {
    let v = hooks::uninstall(dir);
    assert_contract(&v.report);
    v.report
}

fn configured() -> Repo {
    let repo = Repo::new();
    repo.config(CONFIG);
    repo
}

fn assert_installed(dir: &Path) {
    for (name, trigger) in HOOKS {
        let path = dir.join(name);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, hooks::script(name, trigger), "{}", path.display());
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "{}", path.display());
    }
}

fn assert_absent(dir: &Path) {
    for (name, _) in HOOKS {
        assert!(!dir.join(name).exists(), "{name}");
    }
}

#[test]
fn hooks_install_into_dot_git_hooks_and_pass() {
    let repo = configured();
    let report = install(&repo.root(), false);
    assert_eq!(report.status, RunStatus::Pass);
    assert_eq!(
        rules(&report, "garita"),
        ["hook-installed", "hook-installed"]
    );
    assert_eq!(report.examined.get("hooks"), Some(&2));
    assert_installed(&repo.root().join(".git/hooks"));
}

#[test]
fn the_script_fails_closed_and_execs_the_trigger() {
    let text = hooks::script("pre-push", Trigger::Push);
    assert!(text.starts_with("#!/bin/sh\n"));
    assert!(text.contains("command -v garita"));
    assert!(text.ends_with("exec garita run --trigger push\n"));
}

#[test]
fn a_relative_core_hooks_path_is_honoured() {
    let repo = configured();
    repo.git(&["config", "core.hooksPath", ".husky"]);
    install(&repo.root(), false);
    assert_installed(&repo.root().join(".husky"));
    assert_absent(&repo.root().join(".git/hooks"));
}

#[test]
fn an_absolute_core_hooks_path_inside_the_repository_is_honoured() {
    let repo = configured();
    let dir = repo.root().join("tools/hooks");
    repo.git(&["config", "core.hooksPath", dir.to_str().unwrap()]);
    let report = install(&repo.root(), false);
    assert_eq!(report.status, RunStatus::Pass);
    assert_installed(&dir);
}

#[test]
fn an_absolute_core_hooks_path_outside_the_repository_is_refused_unless_replace() {
    let repo = configured();
    let dir = repo.base().join("shared-hooks");
    repo.git(&["config", "core.hooksPath", dir.to_str().unwrap()]);
    let report = install(&repo.root(), false);
    assert_eq!(report.exit_code(), 1);
    assert_eq!(
        finding(&report, "hooks-dir-shared").severity,
        Severity::Error
    );
    assert!(!dir.exists());
    let report = install(&repo.root(), true);
    assert_eq!(report.exit_code(), 0);
    assert_installed(&dir);
}

#[test]
fn from_a_linked_worktree_hooks_go_where_git_reads_them() {
    let repo = configured();
    repo.commit_all("one");
    let wt = repo.base().join("wt");
    repo.git(&["worktree", "add", "-q", wt.to_str().unwrap()]);
    let report = install(&wt, false);
    assert_eq!(report.status, RunStatus::Pass, "{report:#?}");
    let dir = PathBuf::from(
        git_in(
            &wt,
            &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
        )
        .trim(),
    );
    assert_installed(&dir);
    assert_eq!(dir, repo.root().join(".git/hooks"));
}

#[test]
fn installing_twice_is_unchanged() {
    let repo = configured();
    install(&repo.root(), false);
    let report = install(&repo.root(), false);
    assert_eq!(
        rules(&report, "garita"),
        ["hook-unchanged", "hook-unchanged"]
    );
    assert_installed(&repo.root().join(".git/hooks"));
}

fn script_v0(name: &str, trigger: Trigger) -> String {
    format!(
        "#!/bin/sh\n# garita hook format 0: {name}\nexec garita run --trigger {}\n",
        trigger.as_str()
    )
}

#[test]
fn an_exact_older_format_script_is_garitas_in_that_format() {
    let formats: &[(u32, hooks::Script)] = &[(0, script_v0), (1, hooks::script)];
    let old = script_v0("pre-push", Trigger::Push);
    assert_eq!(
        hooks::classify(&old, "pre-push", Trigger::Push, formats),
        Kind::Garita(0)
    );
    let edited = format!("{old}echo more\n");
    assert!(matches!(
        hooks::classify(&edited, "pre-push", Trigger::Push, formats),
        Kind::Modified { .. }
    ));
    let current = hooks::script("pre-push", Trigger::Push);
    assert_eq!(
        hooks::classify(&current, "pre-push", Trigger::Push, hooks::FORMATS),
        Kind::Garita(1)
    );
    assert!(!hooks::classify(&current, "pre-commit", Trigger::Commit, hooks::FORMATS).is_garita());
}

#[test]
fn a_file_that_only_carries_the_marker_is_refused_and_survives_uninstall() {
    let repo = configured();
    let dir = repo.root().join(".git/hooks");
    fs::create_dir_all(&dir).unwrap();
    let mine = "#!/bin/sh\necho mine\n# garita hook format 1: copied\n";
    fs::write(dir.join("pre-commit"), mine).unwrap();
    let report = install(&repo.root(), false);
    assert_eq!(report.exit_code(), 1);
    assert!(has_rule(&report, "hook-foreign"));
    assert_eq!(fs::read_to_string(dir.join("pre-commit")).unwrap(), mine);
    assert!(!dir.join("pre-push").exists());
    uninstall(&repo.root());
    assert_eq!(fs::read_to_string(dir.join("pre-commit")).unwrap(), mine);
}

#[test]
fn garitas_hook_with_a_line_appended_is_foreign_to_install_and_uninstall() {
    let repo = configured();
    install(&repo.root(), false);
    let path = repo.root().join(".git/hooks/pre-push");
    let edited = format!("{}echo extra\n", hooks::script("pre-push", Trigger::Push));
    fs::write(&path, &edited).unwrap();
    let report = install(&repo.root(), false);
    assert_eq!(report.exit_code(), 1);
    assert!(
        finding(&report, "hook-foreign")
            .message
            .contains("has been edited")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
    let report = uninstall(&repo.root());
    assert_eq!(rules(&report, "garita"), ["hook-removed", "hook-foreign"]);
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
}

#[test]
fn a_failed_second_write_restores_the_first_hook() {
    let repo = configured();
    let dir = repo.root().join(".git/hooks");
    fs::create_dir_all(dir.join("pre-push/inside")).unwrap();
    let mine = "#!/bin/sh\necho mine\n";
    fs::write(dir.join("pre-commit"), mine).unwrap();
    fs::set_permissions(dir.join("pre-commit"), fs::Permissions::from_mode(0o700)).unwrap();
    let report = install(&repo.root(), true);
    assert_eq!(report.status, RunStatus::NotRun, "{report:#?}");
    assert_eq!(fs::read_to_string(dir.join("pre-commit")).unwrap(), mine);
    let mode = fs::metadata(dir.join("pre-commit"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    fs::remove_file(dir.join("pre-commit")).unwrap();
    install(&repo.root(), true);
    assert!(!dir.join("pre-commit").exists());
    let leftovers: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".garita-hook-"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn a_refused_default_hooks_dir_does_not_blame_core_hooks_path() {
    let repo = configured();
    let outside = repo.base().join("outside-hooks");
    fs::create_dir_all(&outside).unwrap();
    let hooks_dir = repo.root().join(".git/hooks");
    let _ = fs::remove_dir_all(&hooks_dir);
    symlink(&outside, &hooks_dir).unwrap();
    let report = install(&repo.root(), false);
    let message = &finding(&report, "hooks-dir-shared").message;
    assert!(!message.contains("core.hooksPath"), "{message}");
    assert!(message.contains(".git/hooks resolves to"), "{message}");
    repo.git(&["config", "core.hooksPath", outside.to_str().unwrap()]);
    let report = install(&repo.root(), false);
    assert!(
        finding(&report, "hooks-dir-shared")
            .message
            .contains("(core.hooksPath)")
    );
}

fn has_rule(report: &Report, rule: &str) -> bool {
    rules(report, "garita").contains(&rule)
}

#[test]
fn a_foreign_hook_is_refused_with_nothing_written_then_replaced_with_replace() {
    let repo = configured();
    let dir = repo.root().join(".git/hooks");
    let foreign = "#!/bin/sh\nnpm test\n";
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("pre-push"), foreign).unwrap();
    let report = install(&repo.root(), false);
    assert_eq!(report.exit_code(), 1);
    assert!(report.blocking);
    let f = finding(&report, "hook-foreign");
    assert_eq!(f.severity, Severity::Error);
    assert!(
        f.message.contains("pre-push") && f.message.contains("--replace"),
        "{}",
        f.message
    );
    assert!(!dir.join("pre-commit").exists());
    assert_eq!(fs::read_to_string(dir.join("pre-push")).unwrap(), foreign);
    let report = install(&repo.root(), true);
    assert_eq!(report.exit_code(), 0);
    assert_eq!(
        rules(&report, "garita"),
        ["hook-installed", "hook-replaced"]
    );
    assert_installed(&dir);
}

#[test]
fn a_symlinked_hook_is_refused_and_with_replace_the_link_itself_is_replaced() {
    let repo = configured();
    let dir = repo.root().join(".git/hooks");
    fs::create_dir_all(&dir).unwrap();
    let target = repo.base().join("elsewhere.sh");
    fs::write(&target, "#!/bin/sh\nexit 0\n").unwrap();
    symlink(&target, dir.join("pre-commit")).unwrap();
    let report = install(&repo.root(), false);
    assert_eq!(report.exit_code(), 1);
    assert_eq!(finding(&report, "hook-symlink").severity, Severity::Error);
    assert!(!dir.join("pre-push").exists());
    install(&repo.root(), true);
    let meta = fs::symlink_metadata(dir.join("pre-commit")).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(fs::read_to_string(&target).unwrap(), "#!/bin/sh\nexit 0\n");
    assert_installed(&dir);
}

#[test]
fn without_a_config_hooks_install_with_a_warning() {
    let repo = Repo::new();
    let report = install(&repo.root(), false);
    assert_eq!(report.status, RunStatus::Warn);
    assert_eq!(report.exit_code(), 0);
    assert!(
        finding(&report, "config-missing")
            .message
            .contains("block every commit")
    );
    assert_installed(&repo.root().join(".git/hooks"));
}

#[test]
fn outside_a_repository_install_is_not_run() {
    let repo = Repo::new();
    let report = install(&repo.base().join("bin"), false);
    assert_eq!(report.status, RunStatus::NotRun);
    assert_eq!(report.exit_code(), 2);
}

#[test]
fn uninstall_removes_garitas_hooks_and_leaves_a_foreign_one() {
    let repo = configured();
    install(&repo.root(), false);
    let dir = repo.root().join(".git/hooks");
    fs::write(dir.join("pre-push"), "#!/bin/sh\nnpm test\n").unwrap();
    let report = uninstall(&repo.root());
    assert_eq!(report.status, RunStatus::Pass);
    assert_eq!(rules(&report, "garita"), ["hook-removed", "hook-foreign"]);
    assert!(!dir.join("pre-commit").exists());
    assert_eq!(
        fs::read_to_string(dir.join("pre-push")).unwrap(),
        "#!/bin/sh\nnpm test\n"
    );
}
