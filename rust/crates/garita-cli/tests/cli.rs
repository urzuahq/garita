use std::process::{Command, Output};

fn garita(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_garita"))
        .args(args)
        .output()
        .expect("spawn garita")
}

#[test]
fn version_exits_zero() {
    assert_eq!(garita(&["--version"]).status.code(), Some(0));
}

#[test]
fn unimplemented_commands_exit_one_with_empty_stdout() {
    for args in [&["run"][..], &["doctor"], &["hooks", "install"]] {
        let out = garita(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("is not implemented"), "{args:?}");
    }
}
