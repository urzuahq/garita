#![cfg(unix)]

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use common::TempDir;
use garita_io::process::{Finished, Invocation, Outcome, run};

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn invoke(dir: &Path, parts: &[&str], timeout: Duration) -> Invocation {
    Invocation::new(argv(parts), dir, timeout, &dir.join("out.log"))
}

fn ran(inv: &Invocation) -> Finished {
    match run(inv).unwrap() {
        Outcome::Ran(f) => f,
        other => panic!("expected the tool to run: {other:?}"),
    }
}

fn sh(dir: &Path, script: &str, timeout: Duration) -> Finished {
    ran(&invoke(dir, &["sh", "-c", script], timeout))
}

const LONG: Duration = Duration::from_secs(30);

#[test]
fn records_the_tools_own_exit_code() {
    let tmp = TempDir::new();
    for code in [0, 1, 3] {
        let f = sh(
            tmp.path(),
            &format!("echo out; echo err >&2; exit {code}"),
            LONG,
        );
        assert_eq!(f.exit_code, Some(code));
        assert_eq!(f.signal, None);
        assert!(!f.timed_out);
        assert_eq!(f.stdout, "out\n");
        assert_eq!(f.stderr, "err\n");
        assert!(f.finished >= f.started);
    }
    let file = fs::read_to_string(tmp.path().join("out.log")).unwrap();
    assert!(file.contains("out\n") && file.contains("err\n"), "{file}");
}

#[test]
fn passes_arguments_without_a_shell() {
    let tmp = TempDir::new();
    let f = ran(&invoke(
        tmp.path(),
        &["printf", "%s|", "a b", "$HOME", "*"],
        LONG,
    ));
    assert_eq!(f.stdout, "a b|$HOME|*|");
}

#[test]
fn passes_extra_environment() {
    let tmp = TempDir::new();
    let mut inv = invoke(tmp.path(), &["sh", "-c", "printf %s \"$GARITA_X\""], LONG);
    inv.env.push(("GARITA_X".into(), "yes".into()));
    assert_eq!(ran(&inv).stdout, "yes");
}

#[test]
fn a_missing_program_could_not_start() {
    let tmp = TempDir::new();
    match run(&invoke(tmp.path(), &["garita-no-such-program"], LONG)).unwrap() {
        Outcome::CouldNotStart { reason } => {
            assert!(reason.contains("garita-no-such-program"), "{reason}");
            assert!(reason.to_lowercase().contains("no such file"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_that_is_not_executable_could_not_start() {
    let tmp = TempDir::new();
    let script = tmp.path().join("tool.sh");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let program = script.to_string_lossy().into_owned();
    match run(&invoke(tmp.path(), &[&program], LONG)).unwrap() {
        Outcome::CouldNotStart { reason } => assert!(reason.contains("tool.sh"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_missing_working_directory_could_not_start() {
    let tmp = TempDir::new();
    let mut inv = invoke(tmp.path(), &["true"], LONG);
    inv.cwd = tmp.path().join("gone");
    match run(&inv).unwrap() {
        Outcome::CouldNotStart { reason } => {
            assert!(
                reason.contains("true") && reason.contains("gone"),
                "{reason}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_empty_command_is_an_error() {
    let tmp = TempDir::new();
    assert!(run(&invoke(tmp.path(), &[], LONG)).is_err());
}

#[test]
fn reads_both_full_pipes_without_deadlock() {
    let tmp = TempDir::new();
    let size = 10 * 1024 * 1024;
    let script = format!("head -c {size} /dev/zero & head -c {size} /dev/zero >&2; wait");
    let f = sh(tmp.path(), &script, LONG);
    assert_eq!(f.exit_code, Some(0));
    assert_eq!(f.stdout.len(), size);
    assert_eq!(f.stderr.len(), size);
    assert!(!f.stdout_truncated && !f.stderr_truncated);
    let file = fs::metadata(tmp.path().join("out.log")).unwrap().len();
    assert_eq!(file, 2 * size as u64);
}

#[test]
fn keeps_head_and_tail_past_the_memory_limit() {
    let tmp = TempDir::new();
    let mut inv = invoke(
        tmp.path(),
        &[
            "sh",
            "-c",
            "printf HEAD; head -c 100000 /dev/zero | tr '\\0' x; printf TAIL",
        ],
        LONG,
    );
    inv.memory_limit = 1000;
    let f = ran(&inv);
    assert!(f.stdout_truncated && !f.stderr_truncated);
    assert!(f.stdout.starts_with("HEAD") && f.stdout.ends_with("TAIL"));
    assert!(f.stdout.len() < 1200, "{}", f.stdout.len());
    let file = fs::metadata(tmp.path().join("out.log")).unwrap().len();
    assert_eq!(file, 100_008);
}

#[test]
fn a_tool_past_its_timeout_is_killed() {
    let tmp = TempDir::new();
    let clock = Instant::now();
    let f = ran(&invoke(
        tmp.path(),
        &["sleep", "30"],
        Duration::from_millis(300),
    ));
    assert!(f.timed_out);
    assert_eq!(f.exit_code, None);
    assert!(f.signal.is_some());
    assert!(
        clock.elapsed() < Duration::from_secs(10),
        "{:?}",
        clock.elapsed()
    );
}

fn alive(pid: &str) -> bool {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
    !stat.is_empty() && !stat.starts_with('Z')
}

#[test]
fn a_timeout_kills_the_tools_descendants() {
    let tmp = TempDir::new();
    let f = sh(
        tmp.path(),
        "sleep 60 & echo $!; wait",
        Duration::from_millis(500),
    );
    assert!(f.timed_out);
    let pid = f.stdout.trim().to_string();
    assert!(!pid.is_empty());
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(&pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(&pid), "grandchild {pid} outlived the timeout");
}

#[test]
fn a_tool_that_ignores_sigterm_is_still_killed() {
    let tmp = TempDir::new();
    let clock = Instant::now();
    let f = sh(
        tmp.path(),
        "trap '' TERM; while :; do sleep 0.05; done",
        Duration::from_millis(300),
    );
    assert!(f.timed_out);
    assert_eq!(f.exit_code, None);
    assert_eq!(f.signal, Some(9));
    assert!(
        clock.elapsed() < Duration::from_secs(10),
        "{:?}",
        clock.elapsed()
    );
}

#[test]
fn a_tool_killed_by_another_signal_has_no_exit_code() {
    let tmp = TempDir::new();
    let f = sh(tmp.path(), "kill -9 $$", LONG);
    assert!(!f.timed_out);
    assert_eq!(f.exit_code, None);
    assert_eq!(f.signal, Some(9));
}

#[test]
fn stdin_is_closed() {
    let tmp = TempDir::new();
    let clock = Instant::now();
    let f = ran(&invoke(tmp.path(), &["cat"], LONG));
    assert_eq!(f.exit_code, Some(0));
    assert_eq!(f.stdout, "");
    assert!(clock.elapsed() < Duration::from_secs(10));
}

#[test]
fn non_utf8_output_is_converted_lossily() {
    let tmp = TempDir::new();
    let f = ran(&invoke(tmp.path(), &["printf", "\\377ok"], LONG));
    assert_eq!(f.stdout, "\u{FFFD}ok");
    assert_eq!(fs::read(tmp.path().join("out.log")).unwrap(), b"\xffok");
}

#[test]
fn a_background_child_holding_the_pipe_does_not_hang_the_run() {
    let tmp = TempDir::new();
    let clock = Instant::now();
    let f = sh(tmp.path(), "sleep 60 & echo started", LONG);
    assert_eq!(f.exit_code, Some(0));
    assert!(f.stdout.starts_with("started"));
    assert!(
        clock.elapsed() < Duration::from_secs(15),
        "{:?}",
        clock.elapsed()
    );
}
