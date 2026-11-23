#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use garita_core::receipt::Receipt;
use garita_core::report::{Report, RunStatus, Trigger};
use garita_io::engine::{self, Clock, Outcome, Request};

pub const RECORDINGS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../garita-core/tests/fixtures/recordings"
);

/// A clock that moves one second per reading, so receipts sort in the order the runs happened.
pub struct SteppingClock(Mutex<u64>);

impl SteppingClock {
    pub fn new() -> Self {
        Self(Mutex::new(1_795_230_902))
    }
}

impl Clock for SteppingClock {
    fn now(&self) -> SystemTime {
        let mut secs = self.0.lock().unwrap();
        *secs += 1;
        UNIX_EPOCH + Duration::from_secs(*secs)
    }
}

/// A temporary repository at `<tmp>/repo` with fake tools under `<tmp>/bin`.
pub struct Repo {
    base: PathBuf,
    clock: SteppingClock,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

pub fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

impl Repo {
    pub fn new() -> Self {
        Self::with_branch("main")
    }

    pub fn with_branch(branch: &str) -> Self {
        let base = std::env::temp_dir().join(format!("garita-engine-{}", ulid::Ulid::generate()));
        fs::create_dir_all(base.join("repo")).unwrap();
        fs::create_dir_all(base.join("bin")).unwrap();
        let base = fs::canonicalize(&base).unwrap();
        let repo = Self {
            base,
            clock: SteppingClock::new(),
        };
        repo.git(&["init", "-q", "-b", branch]);
        repo.write(".gitignore", ".garita/receipts/\n.turbo/\n");
        repo
    }

    /// Wraps an existing directory (a clone) as a repository under this temp base.
    pub fn adopt(&self, name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("garita-engine-{}", ulid::Ulid::generate()));
        fs::create_dir_all(&base).unwrap();
        fs::rename(self.base.join(name), base.join("repo")).unwrap();
        fs::create_dir_all(base.join("bin")).unwrap();
        Self {
            base: fs::canonicalize(base).unwrap(),
            clock: SteppingClock::new(),
        }
    }

    pub fn root(&self) -> PathBuf {
        self.base.join("repo")
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn git(&self, args: &[&str]) -> String {
        git_in(&self.root(), args)
    }

    pub fn write(&self, rel: &str, content: &str) {
        let path = self.root().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    pub fn config(&self, yaml: &str) {
        self.write(".garita/config.yaml", yaml);
    }

    pub fn stage(&self, paths: &[&str]) {
        let mut args = vec!["add", "--"];
        args.extend_from_slice(paths);
        self.git(&args);
    }

    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// Writes an executable `sh` script under `<tmp>/bin` and returns its absolute path.
    pub fn tool(&self, name: &str, body: &str) -> String {
        let path = self.base.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// A tool that prints a recorded run and exits with its code.
    pub fn replay(&self, name: &str, case: &str) -> String {
        self.tool(name, &replay_body(case))
    }

    /// A tool that appends `<cwd>|<args>` to a log and exits 0.
    pub fn recorder(&self, name: &str) -> String {
        let log = self.log_path(name);
        self.tool(
            name,
            &format!("printf '%s|%s\\n' \"$PWD\" \"$*\" >> '{}'", log.display()),
        )
    }

    pub fn log_path(&self, name: &str) -> PathBuf {
        self.base.join(format!("{name}.log"))
    }

    pub fn log(&self, name: &str) -> Vec<String> {
        fs::read_to_string(self.log_path(name))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    pub fn request(&self, trigger: Trigger) -> Request {
        Request::new(self.root(), "0.1.0", trigger)
    }

    pub fn run(&self) -> Outcome {
        self.run_request(&self.request(Trigger::Commit))
    }

    pub fn run_at(&self, trigger: Trigger) -> Outcome {
        self.run_request(&self.request(trigger))
    }

    pub fn run_named(&self, trigger: Trigger, checks: &[&str]) -> Outcome {
        let mut request = self.request(trigger);
        request.checks = checks.iter().map(|c| (*c).to_string()).collect();
        self.run_request(&request)
    }

    pub fn run_request(&self, request: &Request) -> Outcome {
        let outcome = engine::run(request, &self.clock);
        assert_contract(&outcome.report);
        outcome
    }

    pub fn receipts(&self) -> Vec<PathBuf> {
        let dir = self.root().join(".garita/receipts");
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        files.retain(|p| p.extension().is_some_and(|e| e == "json"));
        files.sort();
        files
    }

    pub fn receipt_of(&self, report: &Report) -> Receipt {
        let rel = report
            .receipt
            .as_deref()
            .expect("the report names its receipt");
        let text = fs::read_to_string(self.root().join(rel)).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

pub fn recording(case: &str) -> PathBuf {
    Path::new(RECORDINGS).join(case)
}

pub fn replay_body(case: &str) -> String {
    let dir = recording(case);
    let code = fs::read_to_string(dir.join("exit_code")).unwrap();
    format!(
        "cat '{d}/stdout'\ncat '{d}/stderr' >&2\nexit {}",
        code.trim(),
        d = dir.display()
    )
}

/// The contract invariants every report must hold (RFC-1 §5).
pub fn assert_contract(report: &Report) {
    assert_eq!(
        report.blocking,
        report.exit_code() == 1,
        "blocking exactly when the exit code is 1: {report:#?}"
    );
    assert_eq!(
        report.next_action_reason.is_none(),
        report.next_action.is_none(),
        "next_action_reason is null exactly when next_action is: {report:#?}"
    );
    if report.status == RunStatus::NotRun {
        assert!(
            report.error.is_some(),
            "a not-run report says why: {report:#?}"
        );
    }
    let json = engine::to_json(report);
    assert!(!json.contains('\n'));
    let back: Report = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, report);
}

pub fn check<'a>(report: &'a Report, name: &str) -> &'a garita_core::report::CheckEntry {
    report
        .checks
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no check {name} in {report:#?}"))
}

pub fn rules<'a>(report: &'a Report, check: &str) -> Vec<&'a str> {
    report
        .findings
        .iter()
        .filter(|f| f.check == check)
        .map(|f| f.rule.as_str())
        .collect()
}

pub fn finding<'a>(report: &'a Report, rule: &str) -> &'a garita_core::report::Finding {
    report
        .findings
        .iter()
        .find(|f| f.rule == rule)
        .unwrap_or_else(|| panic!("no {rule} finding in {report:#?}"))
}
