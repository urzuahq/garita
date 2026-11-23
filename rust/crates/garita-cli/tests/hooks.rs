use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use garita_core::report::{Report, RunStatus};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A repository at `<tmp>/repo` and a `PATH` directory at `<tmp>/bin` holding only `git`, and
/// `garita` when asked for.
struct Sandbox(PathBuf);

impl Sandbox {
    fn new(with_garita: bool) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("garita-cli-hooks-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("repo")).unwrap();
        fs::create_dir_all(dir.join("bin")).unwrap();
        let dir = fs::canonicalize(dir).unwrap();
        symlink(real_git(), dir.join("bin/git")).unwrap();
        if with_garita {
            symlink(env!("CARGO_BIN_EXE_garita"), dir.join("bin/garita")).unwrap();
        }
        let s = Self(dir);
        s.git(&["init", "-q", "-b", "main"]).assert_ok();
        s.write(".gitignore", ".garita/receipts/\n");
        s
    }

    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.repo().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// A lint check whose tool exits with `code`.
    fn lint_exits(&self, code: i32) {
        let tool = self.0.join("lint");
        fs::write(
            &tool,
            format!("#!/bin/sh\necho \"lint says {code}\" >&2\nexit {code}\n"),
        )
        .unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        self.write(
            ".garita/config.yaml",
            &format!(
                "schema_version: 1\nchecks:\n  lint:\n    run: {} {{files}}\n",
                tool.display()
            ),
        );
    }

    fn env(&self, cmd: &mut Command) {
        cmd.current_dir(self.repo())
            .env("PATH", self.0.join("bin"))
            .env("HOME", &self.0)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
    }

    fn git(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(self.0.join("bin/git"));
        cmd.args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args);
        self.env(&mut cmd);
        cmd.output().unwrap()
    }

    fn garita(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_garita"));
        cmd.args(args);
        self.env(&mut cmd);
        cmd.output().unwrap()
    }

    fn install(&self) -> Report {
        let out = self.garita(&["hooks", "install"]);
        let report = report_of(&out);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        report
    }

    fn commit(&self, file: &str) -> Output {
        self.write(file, "x\n");
        self.git(&["add", file]).assert_ok();
        self.git(&["commit", "-q", "-m", file])
    }

    fn commits(&self) -> usize {
        let out = self.git(&["rev-list", "--count", "--all"]);
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .unwrap_or(0)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

trait AssertOk {
    fn assert_ok(&self);
}

impl AssertOk for Output {
    fn assert_ok(&self) {
        assert!(self.status.success(), "{}", stderr(self));
    }
}

fn real_git() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join("git"))
        .find(|p| p.is_file())
        .expect("git on the test's PATH")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn report_of(out: &Output) -> Report {
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    assert_eq!(stdout.matches('\n').count(), 1, "{stdout:?}");
    let report: Report = serde_json::from_str(&stdout).unwrap();
    assert_eq!(i32::from(report.exit_code()), out.status.code().unwrap());
    assert_eq!(report.blocking, report.exit_code() == 1);
    assert_eq!(
        report.next_action.is_none(),
        report.next_action_reason.is_none()
    );
    if report.status == RunStatus::NotRun {
        assert!(report.error.is_some());
    }
    report
}

#[test]
fn the_installed_pre_commit_hook_rejects_a_failing_commit_and_accepts_a_passing_one() {
    let s = Sandbox::new(true);
    s.lint_exits(1);
    s.install();
    let out = s.commit("a.ts");
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("garita: fail at commit"),
        "{}",
        stderr(&out)
    );
    assert_eq!(s.commits(), 0);
    s.lint_exits(0);
    s.commit("a.ts").assert_ok();
    assert_eq!(s.commits(), 1);
}

#[test]
fn without_garita_on_path_the_hook_rejects_the_commit_with_a_message() {
    let s = Sandbox::new(false);
    s.lint_exits(0);
    let out = Command::new(env!("CARGO_BIN_EXE_garita"))
        .args(["hooks", "install"])
        .current_dir(s.repo())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let out = s.commit("a.ts");
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("which is not on PATH"),
        "{}",
        stderr(&out)
    );
    assert_eq!(s.commits(), 0);
}

#[test]
fn the_pre_push_hook_run_directly_exits_non_zero_on_a_failing_check() {
    let s = Sandbox::new(true);
    s.lint_exits(1);
    s.install();
    s.write("a.ts", "x\n");
    s.git(&["add", "-A"]).assert_ok();
    s.git(&["commit", "-q", "--no-verify", "-m", "one"])
        .assert_ok();
    let hook = s.repo().join(".git/hooks/pre-push");
    let mut cmd = Command::new(&hook);
    cmd.args(["origin", "file:///nowhere"]);
    s.env(&mut cmd);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let report = report_of(&out);
    assert_eq!(report.status, RunStatus::Fail);
    assert_eq!(report.trigger.map(|t| t.as_str()), Some("push"));
}

#[test]
fn doctor_prints_one_report_and_exits_2_without_a_config() {
    let s = Sandbox::new(true);
    let out = s.garita(&["doctor"]);
    let report = report_of(&out);
    assert_eq!(out.status.code(), Some(2));
    assert!(report.error.unwrap().contains("config.yaml"));
    assert!(
        stderr(&out).starts_with("garita doctor: not-run"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn doctor_passes_once_hooks_and_config_are_in_place() {
    let s = Sandbox::new(true);
    s.lint_exits(0);
    s.install();
    let out = s.garita(&["doctor"]);
    let report = report_of(&out);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(report.examined.get("hooks"), Some(&2));
}
