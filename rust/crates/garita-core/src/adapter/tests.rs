use super::*;

fn run(adapter: Adapter, stdout: &str, stderr: &str, exit: Option<i32>) -> Parsed {
    let output = ToolOutput {
        stdout,
        stderr,
        exit_code: exit,
    };
    adapter.parse(&output, &ParseContext::default())
}

fn crashed(parsed: &Parsed) -> bool {
    matches!(parsed.outcome, Outcome::Crashed { .. })
}

#[test]
fn rebase_joins_and_normalises() {
    assert_eq!(rebase("", "src/a.ts"), "src/a.ts");
    assert_eq!(rebase("", "./src/a.ts"), "src/a.ts");
    assert_eq!(rebase("packages/app", "src/a.ts"), "packages/app/src/a.ts");
    assert_eq!(
        rebase("packages/app/", "./src//a.ts"),
        "packages/app/src/a.ts"
    );
    assert_eq!(rebase("./packages/app", "../lib/x.ts"), "packages/lib/x.ts");
    assert_eq!(
        rebase("packages\\app", "src\\a.ts"),
        "packages/app/src/a.ts"
    );
    assert_eq!(rebase("app", "../../outside.ts"), "../outside.ts");
    assert_eq!(rebase("app", "/abs/x.ts"), "/abs/x.ts");
    assert_eq!(rebase("app", "C:\\abs\\x.ts"), "C:/abs/x.ts");
    assert_eq!(rebase("app", "."), "app");
    assert_eq!(rebase("", ""), ".");
}

#[test]
fn fingerprint_is_stable() {
    assert_eq!(
        fingerprint(
            "oxlint/eslint/no-debugger",
            Some("packages/app/src/lint-error.ts"),
            "`debugger` statement is not allowed (help: Remove the debugger statement)",
        ),
        "af1545e241892d99"
    );
    assert_eq!(
        fingerprint(
            "tsc/TS5058",
            None,
            "The specified path does not exist: 'packages/missing'."
        ),
        "c4f64e7946187b69"
    );
    assert_eq!(fingerprint("", Some(""), ""), "d8cbee186ba177a1");
    assert_ne!(fingerprint("r", None, "m"), fingerprint("r", Some(""), "m"));
    assert_ne!(
        fingerprint("ab", Some("c"), "m"),
        fingerprint("a", Some("bc"), "m")
    );
}

#[test]
fn fingerprint_ignores_position() {
    let finding = |line| ToolFinding {
        severity: Severity::Error,
        rule: "tsc/TS2322".into(),
        file: Some("a.ts".into()),
        line: NonZeroU64::new(line),
        column: NonZeroU64::new(3),
        message: "m".into(),
        fix: None,
    };
    assert_eq!(finding(1).fingerprint(), finding(90).fingerprint());
    let full = finding(1).into_finding("types");
    assert_eq!(full.check, "types");
    assert_eq!(full.fingerprint, Some(finding(2).fingerprint()));
}

#[test]
fn registry_names_every_adapter() {
    for adapter in Adapter::ALL {
        assert_eq!(Adapter::from_parser_name(adapter.name()), Ok(adapter));
    }
    assert_eq!(Adapter::Oxlint.kind(), CheckKind::Lint);
    assert_eq!(Adapter::Oxfmt.kind(), CheckKind::Format);
    assert_eq!(Adapter::Tsc.kind(), CheckKind::Types);
    assert_eq!(Adapter::ExitCode.kind(), CheckKind::Other);
    let error = Adapter::from_parser_name("eslint").unwrap_err();
    assert_eq!(error.name, "eslint");
    assert!(error.to_string().contains("unknown parser `eslint`"));
}

#[test]
fn oxlint_rule_names() {
    assert_eq!(
        oxlint::rule(Some("eslint(no-debugger)")),
        "oxlint/eslint/no-debugger"
    );
    assert_eq!(
        oxlint::rule(Some("typescript-eslint(no-explicit-any)")),
        "oxlint/typescript-eslint/no-explicit-any"
    );
    assert_eq!(oxlint::rule(Some("weird")), "oxlint/weird");
    assert_eq!(oxlint::rule(Some("a(b(c))")), "oxlint/a/b(c)");
    assert_eq!(oxlint::rule(Some("(x)")), "oxlint/(x)");
    assert_eq!(oxlint::rule(None), "oxlint/unknown");
}

#[test]
fn oxlint_rebases_and_fixes() {
    let json = r#"{"diagnostics":[{"message":"m","code":"eslint(x)","severity":"warning","filename":"src/my file.ts","labels":[]}],"number_of_files":1}"#;
    let output = ToolOutput {
        stdout: json,
        stderr: "",
        exit_code: Some(0),
    };
    let context = ParseContext {
        cwd: "packages/app",
        files: None,
    };
    let parsed = Adapter::Oxlint.parse(&output, &context);
    let finding = &parsed.findings[0];
    assert_eq!(finding.file.as_deref(), Some("packages/app/src/my file.ts"));
    assert_eq!(
        finding.fix.as_deref(),
        Some("oxlint --fix 'packages/app/src/my file.ts'")
    );
    assert_eq!(finding.line, None);
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.message, "m");
}

/// The single finding a non-zero exit with unreadable output gives.
fn assert_unrecognized(parsed: &Parsed, tool: &str, exit: i32) {
    assert_eq!(parsed.outcome, Outcome::Findings, "{tool}");
    assert_eq!(parsed.findings.len(), 1, "{tool}");
    let finding = &parsed.findings[0];
    assert_eq!(finding.rule, format!("{tool}/unrecognized-output"));
    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.file, None);
    assert!(
        finding
            .message
            .starts_with(&format!("{tool} exited with code {exit}")),
        "{}",
        finding.message
    );
}

#[test]
fn every_parser_blocks_on_a_failed_exit_with_empty_output() {
    for (adapter, tool) in [
        (Adapter::Oxlint, "oxlint"),
        (Adapter::Oxfmt, "oxfmt"),
        (Adapter::Tsc, "tsc"),
    ] {
        let parsed = run(adapter, "", "", Some(1));
        assert_unrecognized(&parsed, tool, 1);
        assert_eq!(
            parsed.findings[0].message,
            format!("{tool} exited with code 1 and printed nothing")
        );
    }
    let parsed = run(Adapter::ExitCode, "", "", Some(1));
    assert_eq!(parsed.findings[0].rule, "exit-code/failed");
}

#[test]
fn every_parser_crashes_when_killed() {
    for adapter in Adapter::ALL {
        let parsed = run(adapter, "anything", "", None);
        assert!(crashed(&parsed), "{}", adapter.name());
        assert!(parsed.findings.is_empty());
    }
}

#[test]
fn unrecognized_message_keeps_a_clipped_tail() {
    let stdout = format!("{}\nlast words\n", "é".repeat(1_000_000));
    let parsed = run(Adapter::Tsc, &stdout, "", Some(2));
    assert_unrecognized(&parsed, "tsc", 2);
    let message = &parsed.findings[0].message;
    assert!(message.ends_with("\nlast words"), "{message}");
    assert!(message.len() < 400, "{}", message.len());
}

#[test]
fn oxlint_unreadable_output() {
    for stdout in [
        "",
        "{",
        "{\"diagnostics\": [",
        "\u{0}\u{1}\u{fffd}garbage",
        "[]",
        "{}",
    ] {
        assert_eq!(
            run(Adapter::Oxlint, stdout, "", Some(0)).outcome,
            Outcome::Clean,
            "{stdout:?}: a tool that exits 0 without diagnostics passed by its own word"
        );
        assert_unrecognized(&run(Adapter::Oxlint, stdout, "", Some(1)), "oxlint", 1);
    }
    let clean = r#"{"diagnostics":[],"number_of_files":2}"#;
    assert!(crashed(&run(Adapter::Oxlint, clean, "", None)));
    let silent_failure = run(Adapter::Oxlint, clean, "", Some(1));
    assert_unrecognized(&silent_failure, "oxlint", 1);
    assert_eq!(silent_failure.examined.get(FILES), Some(&2));
    assert_eq!(
        run(Adapter::Oxlint, clean, "", Some(0)).outcome,
        Outcome::Clean
    );
    let crlf = "No files found to lint.\r\n{\"diagnostics\":[],\r\n\"number_of_files\":0}\r\n";
    assert_eq!(
        run(Adapter::Oxlint, crlf, "", Some(1)).outcome,
        Outcome::Nothing
    );
}

#[test]
fn oxlint_text_styles_give_severity_rule_and_place() {
    let graphical = "\n  ! eslint(eqeqeq): Expected ===\n   ,-[src/w.ts:2:12]\n 2 |   a == b;\n   `----\n  help: Prefer ===\n\n  x eslint(no-debugger): no debugger\n   ,-[src/e.ts:7:3]\n   `----\nFound 1 warning and 1 error.\nFinished in 4ms on 9 files using 2 threads.\n";
    let context = ParseContext {
        cwd: "packages/app",
        files: None,
    };
    let output = ToolOutput {
        stdout: graphical,
        stderr: "",
        exit_code: Some(1),
    };
    let parsed = Adapter::Oxlint.parse(&output, &context);
    let got: Vec<_> = parsed
        .findings
        .iter()
        .map(|f| {
            (
                f.severity,
                f.rule.as_str(),
                f.file.as_deref(),
                f.line.map(|l| l.get()),
                f.column.map(|c| c.get()),
                f.message.as_str(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                Severity::Warning,
                "oxlint/eslint/eqeqeq",
                Some("packages/app/src/w.ts"),
                Some(2),
                Some(12),
                "Expected === (help: Prefer ===)"
            ),
            (
                Severity::Error,
                "oxlint/eslint/no-debugger",
                Some("packages/app/src/e.ts"),
                Some(7),
                Some(3),
                "no debugger"
            ),
        ]
    );
    assert_eq!(parsed.examined.get(FILES), Some(&9));
    let one_line = "src/a.ts:4:2: warning eslint(eqeqeq): m help: h\n";
    let parsed = run(Adapter::Oxlint, one_line, "", Some(0));
    assert_eq!(parsed.findings[0].severity, Severity::Warning);
    assert_eq!(parsed.findings[0].message, "m (help: h)");
    assert_eq!(parsed.findings[0].column.map(|c| c.get()), Some(2));
    let other = run(Adapter::Oxlint, "prettier: all good\n", "", Some(0));
    assert_eq!(other.outcome, Outcome::Clean);
    assert_unrecognized(
        &run(Adapter::Oxlint, "tsup: build failed\n", "", Some(1)),
        "oxlint",
        1,
    );
}

const ONE_DIAGNOSTIC: &str = r#"{"diagnostics":[{"message":"m","code":"eslint(no-debugger)","severity":"error","filename":"src/a.ts","labels":[{"span":{"line":4,"column":2}}]}],"number_of_files":3}"#;

#[test]
fn oxlint_reads_only_the_first_json_value() {
    for after in [
        "\nFinished in 4ms.\n",
        "\n{\"another\": 1}\n",
        "{not json",
        "\n}",
    ] {
        let stdout = format!("{ONE_DIAGNOSTIC}{after}");
        let parsed = run(Adapter::Oxlint, &stdout, "", Some(1));
        assert_eq!(parsed.outcome, Outcome::Findings, "{after:?}");
        assert_eq!(parsed.findings.len(), 1);
        assert_eq!(parsed.examined.get(FILES), Some(&3));
    }
}

#[test]
fn oxlint_diagnostics_win_over_no_files_prose() {
    let stdout = format!(
        "No files found to lint. Please check your paths and ignore patterns.\n{ONE_DIAGNOSTIC}"
    );
    let parsed = run(Adapter::Oxlint, &stdout, "", Some(1));
    assert_eq!(parsed.outcome, Outcome::Findings);
    assert_eq!(parsed.findings[0].file.as_deref(), Some("src/a.ts"));
}

#[test]
fn oxlint_ansi_coloured_output() {
    let stdout = format!(
        "\u{1b}[33mNo files\u{1b}[0m?\n\u{1b}]8;;https://x\u{7}\u{1b}[1m{ONE_DIAGNOSTIC}\u{1b}[0m\n"
    );
    let parsed = run(Adapter::Oxlint, &stdout, "", Some(1));
    assert_eq!(parsed.outcome, Outcome::Findings);
    assert_eq!(parsed.findings[0].rule, "oxlint/eslint/no-debugger");
    assert_eq!(parsed.findings[0].line, NonZeroU64::new(4));
}

#[test]
fn fix_commands_guard_a_leading_dash() {
    let json = r#"{"diagnostics":[{"message":"m","code":"eslint(x)","severity":"error","filename":"-rf.ts","labels":[]}],"number_of_files":1}"#;
    let lint = run(Adapter::Oxlint, json, "", Some(1));
    assert_eq!(lint.findings[0].file.as_deref(), Some("-rf.ts"));
    assert_eq!(
        lint.findings[0].fix.as_deref(),
        Some("oxlint --fix ./-rf.ts")
    );
    let format = run(Adapter::Oxfmt, "--write me.ts\n", "", Some(1));
    assert_eq!(format.findings[0].file.as_deref(), Some("--write me.ts"));
    assert_eq!(
        format.findings[0].fix.as_deref(),
        Some("oxfmt './--write me.ts'")
    );
    let inside = run(Adapter::Oxfmt, "src/-a.ts\n", "", Some(1));
    assert_eq!(inside.findings[0].fix.as_deref(), Some("oxfmt src/-a.ts"));
}

#[test]
fn oxlint_large_output() {
    let one = r#"{"message":"m","code":"eslint(x)","severity":"error","filename":"a.ts","labels":[{"span":{"line":1,"column":1}}]}"#;
    let many = vec![one; 50_000].join(",");
    let stdout = format!("{{\"diagnostics\":[{many}],\"number_of_files\":1}}");
    assert!(stdout.len() > 5_000_000);
    let parsed = run(Adapter::Oxlint, &stdout, "", Some(1));
    assert_eq!(parsed.findings.len(), 50_000);
}

#[test]
fn oxfmt_duration_and_counts() {
    let stdout = "Checking formatting...\r\n\r\nsrc/a (copy).ts (12ms)\r\nsrc/b c.ts (1.5s)\r\n\r\nFormat issues found in above 2 files. Run without `--check` to fix.\r\nFinished in 3ms on 1 file using 2 threads.\r\n";
    let parsed = run(Adapter::Oxfmt, stdout, "", Some(1));
    assert_eq!(parsed.outcome, Outcome::Findings);
    let files: Vec<_> = parsed
        .findings
        .iter()
        .filter_map(|f| f.file.as_deref())
        .collect();
    assert_eq!(files, ["src/a (copy).ts", "src/b c.ts"]);
    assert_eq!(
        parsed.findings[1].fix.as_deref(),
        Some("oxfmt 'src/b c.ts'")
    );
    assert_eq!(parsed.examined.get(FILES), Some(&1));
    assert_eq!(
        oxfmt::finished_count("Finished in 3ms on 12 files using 2 threads."),
        Some(12)
    );
    assert_eq!(oxfmt::finished_count("Finished in 3ms on many files"), None);
}

#[test]
fn oxfmt_unreadable_output() {
    let stdout = "Checking formatting...\n\na.ts (0ms)\n\nFormat issues found in above 3 files.\n";
    assert_unrecognized(&run(Adapter::Oxfmt, stdout, "", Some(1)), "oxfmt", 1);
    let truncated = "Checking formatting...\n\na.ts (0ms)\n";
    assert_unrecognized(&run(Adapter::Oxfmt, truncated, "", Some(1)), "oxfmt", 1);
    assert!(crashed(&run(Adapter::Oxfmt, "\u{0}\u{7f}", "", Some(0))));
    assert!(crashed(&run(
        Adapter::Oxfmt,
        "Checking formatting...\n",
        "",
        Some(0)
    )));
    assert_eq!(run(Adapter::Oxfmt, "", "", Some(0)).outcome, Outcome::Clean);
    let big = "x".repeat(5_000_000);
    let parsed = run(Adapter::Oxfmt, &big, &big, Some(2));
    assert_unrecognized(&parsed, "oxfmt", 2);
    assert!(parsed.findings[0].message.len() < 1000);
    let Outcome::Crashed { reason } = run(Adapter::Oxfmt, &big, "", Some(0)).outcome else {
        panic!("exit 0 with garbage must crash");
    };
    assert!(reason.len() < 400, "{}", reason.len());
}

#[test]
fn tsc_hostile_lines() {
    let stdout = "src/my dir (old)/a b.ts(3,7): error TS2304: Cannot find name 'x'.\r\n  first\r\n    second\r\nnot a diagnostic(1,2): nope\r\nfoo(1,x): error TS1: no\r\n";
    let parsed = run(Adapter::Tsc, stdout, "", Some(2));
    assert_eq!(parsed.outcome, Outcome::Findings);
    assert_eq!(parsed.findings.len(), 1);
    let finding = &parsed.findings[0];
    assert_eq!(finding.file.as_deref(), Some("src/my dir (old)/a b.ts"));
    assert_eq!(finding.line, NonZeroU64::new(3));
    assert_eq!(finding.column, NonZeroU64::new(7));
    assert_eq!(finding.rule, "tsc/TS2304");
    assert_eq!(
        finding.message,
        "Cannot find name 'x'.\n  first\n    second"
    );
    assert_eq!(finding.fix, None);
}

const PRETTY: &str = "\u{1b}[96msrc/a b.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m14\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2322: \u{1b}[0mType 'string' is not assignable to type 'number'.

\u{1b}[7m1\u{1b}[0m export const x: number = \"a\";
\u{1b}[7m \u{1b}[0m \u{1b}[91m             ~\u{1b}[0m

src/b.ts:5:3 - error TS2304: Cannot find name 'y'.
  The name is missing.

5   y;
    ~

Found 2 errors in 2 files.

Errors  Files
     1  src/a b.ts\u{1b}[90m:1\u{1b}[0m
     1  src/b.ts\u{1b}[90m:5\u{1b}[0m
";

#[test]
fn tsc_pretty_format_with_colours() {
    let context = ParseContext {
        cwd: "packages/app",
        files: None,
    };
    let output = ToolOutput {
        stdout: PRETTY,
        stderr: "",
        exit_code: Some(2),
    };
    let parsed = Adapter::Tsc.parse(&output, &context);
    assert_eq!(parsed.outcome, Outcome::Findings);
    assert_eq!(parsed.findings.len(), 2);
    let first = &parsed.findings[0];
    assert_eq!(first.file.as_deref(), Some("packages/app/src/a b.ts"));
    assert_eq!(
        (first.line, first.column),
        (NonZeroU64::new(1), NonZeroU64::new(14))
    );
    assert_eq!(first.rule, "tsc/TS2322");
    assert_eq!(
        first.message,
        "Type 'string' is not assignable to type 'number'."
    );
    let second = &parsed.findings[1];
    assert_eq!(second.file.as_deref(), Some("packages/app/src/b.ts"));
    assert_eq!(second.line, NonZeroU64::new(5));
    assert_eq!(
        second.message,
        "Cannot find name 'y'.\n  The name is missing."
    );
}

#[test]
fn tsc_plain_format_with_colours() {
    let stdout = "\u{1b}[31msrc/a.ts(2,3): error TS1005: ';' expected.\u{1b}[0m\n";
    let parsed = run(Adapter::Tsc, stdout, "", Some(1));
    assert_eq!(parsed.findings[0].file.as_deref(), Some("src/a.ts"));
    assert_eq!(parsed.findings[0].message, "';' expected.");
}

#[test]
fn tsc_severity_words_and_examined() {
    let stdout = "a.ts(1,1): warning TS6133: unused\nb.ts(2,2): suggestion TS80001: maybe\n";
    let output = ToolOutput {
        stdout,
        stderr: "",
        exit_code: Some(0),
    };
    let context = ParseContext {
        cwd: "",
        files: Some(4),
    };
    let parsed = Adapter::Tsc.parse(&output, &context);
    assert_eq!(parsed.findings[0].severity, Severity::Warning);
    assert_eq!(parsed.findings[1].severity, Severity::Info);
    assert_eq!(parsed.examined.get(FILES), Some(&4));
    let none = ParseContext {
        cwd: "",
        files: Some(0),
    };
    let empty = ToolOutput {
        stdout: "",
        stderr: "",
        exit_code: Some(0),
    };
    assert_eq!(Adapter::Tsc.parse(&empty, &none).outcome, Outcome::Nothing);
}

#[test]
fn tsc_hostile_input() {
    assert_eq!(run(Adapter::Tsc, "", "", Some(0)).outcome, Outcome::Clean);
    assert!(run(Adapter::Tsc, "", "", Some(0)).examined.is_empty());
    assert_unrecognized(&run(Adapter::Tsc, "\u{0}garbage", "", Some(2)), "tsc", 2);
    assert!(crashed(&run(
        Adapter::Tsc,
        "a.ts(1,1): error TS1: x",
        "",
        None
    )));
    let line = "src/a.ts(1,14): error TS2322: Type 'string' is not assignable to type 'number'.\n";
    let big = line.repeat(5_000_000 / line.len() + 1);
    let parsed = run(Adapter::Tsc, &big, "", Some(2));
    assert_eq!(parsed.findings.len(), 5_000_000 / line.len() + 1);
    let huge_line = format!("{}(1,1): error TS1: x", "(".repeat(5_000_000));
    assert_eq!(run(Adapter::Tsc, &huge_line, "", Some(2)).findings.len(), 1);
}

#[test]
fn tsc_adversarial_line_is_linear() {
    for unit in [
        "): ",
        "(1,1): error TS",
        "error TS1",
        ":1:1 - error TS9x",
        "1",
    ] {
        let line = format!("{}\n", unit.repeat(5_000_000 / unit.len()));
        let started = std::time::Instant::now();
        let parsed = run(Adapter::Tsc, &line, "", Some(2));
        let took = started.elapsed();
        assert!(took.as_secs() < 5, "{unit:?} took {took:?}");
        assert_eq!(parsed.findings.len(), 1, "{unit:?}");
    }
}

#[test]
fn strip_ansi_sequences() {
    assert_eq!(strip_ansi("plain"), "plain");
    assert_eq!(strip_ansi("\u{1b}[1;31mred\u{1b}[0m"), "red");
    assert_eq!(strip_ansi("a\u{1b}]8;;http://x\u{1b}\\b"), "ab");
    assert_eq!(strip_ansi("a\u{1b}(Bb\u{1b}"), "aBb");
    assert_eq!(strip_ansi("x\u{1b}[12;"), "x");
}

#[test]
fn tsc_counts_project_files() {
    let list = "/w/node_modules/typescript/lib/lib.es5.d.ts\r\n/w/node_modules/x/lib/lib.dom.d.ts\n/w/src/lib/lib.custom.d.ts\n/w/src/a.ts\n\n";
    assert_eq!(count_project_files(list), 2);
    assert_eq!(count_project_files(""), 0);
}

#[test]
fn exit_code_outcomes() {
    let pass = run(Adapter::ExitCode, "ok", "", Some(0));
    assert_eq!(pass.outcome, Outcome::Clean);
    assert!(crashed(&run(Adapter::ExitCode, "", "", None)));
    let fail = run(Adapter::ExitCode, "out 1\nout 2\n", "err 1\n", Some(3));
    assert_eq!(fail.outcome, Outcome::Findings);
    let finding = &fail.findings[0];
    assert_eq!(finding.rule, "exit-code/failed");
    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.file, None);
    assert_eq!(
        finding.message,
        "exited with code 3:\nstderr:\nerr 1\nstdout:\nout 1\nout 2"
    );
    let silent = run(Adapter::ExitCode, "", "\n\n", Some(1));
    assert_eq!(
        silent.findings[0].message,
        "exited with code 1 and printed nothing"
    );
}

#[test]
fn exit_code_tail_is_capped() {
    let many: String = (0..100).map(|i| format!("line {i}\r\n")).collect();
    let tail = exit_code::tail("", &many);
    assert_eq!(tail.lines().count(), 10);
    assert!(tail.starts_with("line 90\n") && tail.ends_with("line 99"));
    let wide = "é".repeat(3_000_000);
    let tail = exit_code::tail(&wide, "");
    assert!(tail.len() <= 4000 && tail.chars().all(|c| c == 'é'));
    let output = ToolOutput {
        stdout: "",
        stderr: "boom",
        exit_code: Some(1),
    };
    let context = ParseContext {
        cwd: "",
        files: Some(0),
    };
    let parsed = Adapter::ExitCode.parse(&output, &context);
    assert_eq!(parsed.examined.get(FILES), Some(&0));
    assert_eq!(parsed.outcome, Outcome::Findings);
}

#[test]
fn turbo_clean_log() {
    use turbo::clean_log;
    assert_eq!(clean_log("$ tsc --noEmit\n"), "");
    assert_eq!(
        clean_log(
            "$ tsc\nsrc/a.ts(1,1): error TS1: x\n[ELIFECYCLE] Command failed with exit code 2.\n"
        ),
        "src/a.ts(1,1): error TS1: x\n"
    );
    assert_eq!(
        clean_log("$ x\r\nline\r\n ELIFECYCLE  Command failed with exit code 1.\r\n"),
        "line\r\n"
    );
    assert_eq!(
        clean_log("tool line\n$ not an echo\n"),
        "tool line\n$ not an echo\n"
    );
    assert_eq!(
        clean_log("[ELIFECYCLE] Command failed with exit code 1."),
        ""
    );
    assert_eq!(clean_log(""), "");
}

#[test]
fn turbo_summary_errors() {
    assert!(turbo::parse_summary("").is_err());
    assert!(turbo::parse_summary("{\"tasks\": [").is_err());
    assert!(turbo::parse_summary("\u{0}").is_err());
    let odd = r#"{"tasks":[{"taskId":"a#b","task":"b","package":"a","directory":"","cache":{"status":"REMOTE-ISH"}}]}"#;
    let summary = turbo::parse_summary(odd).unwrap();
    assert_eq!(summary.tasks[0].cache, turbo::CacheStatus::Unknown);
    assert!(!summary.tasks[0].ran_fresh());
    assert_eq!(summary.exit_code, None);
}

#[test]
fn a_nonzero_exit_with_only_warnings_still_blocks() {
    let stdout = r#"{"diagnostics":[{"message":"m","code":"eslint(no-console)","severity":"warning","filename":"a.ts","labels":[]}],"number_of_files":3}"#;
    let parsed = Adapter::Oxlint.parse(
        &ToolOutput {
            stdout,
            stderr: "",
            exit_code: Some(1),
        },
        &ParseContext::default(),
    );
    assert_eq!(parsed.outcome, Outcome::Findings);
    assert_eq!(parsed.findings.len(), 2);
    let last = parsed
        .findings
        .last()
        .map(|f| (f.rule.as_str(), f.severity));
    assert_eq!(last, Some(("oxlint/failed", Severity::Error)));

    let same_at_zero = Adapter::Oxlint.parse(
        &ToolOutput {
            stdout,
            stderr: "",
            exit_code: Some(0),
        },
        &ParseContext::default(),
    );
    assert_eq!(same_at_zero.findings.len(), 1);
}

#[test]
fn a_nonzero_exit_with_only_tsc_messages_still_blocks() {
    let parsed = Adapter::Tsc.parse(
        &ToolOutput {
            stdout: "a.ts(1,1): message TS6194: x\n",
            stderr: "",
            exit_code: Some(1),
        },
        &ParseContext::default(),
    );
    assert!(parsed.findings.iter().any(|f| f.rule == "tsc/failed"));
    assert!(
        parsed
            .findings
            .iter()
            .any(|f| f.severity == Severity::Error)
    );
}

#[test]
fn every_parser_blocks_or_reports_nothing_on_any_nonzero_exit() {
    let outputs = ["", " \n", "garbage", "{}", "[]", "Checking formatting...\n"];
    for adapter in Adapter::ALL {
        for stdout in outputs {
            let parsed = adapter.parse(
                &ToolOutput {
                    stdout,
                    stderr: "",
                    exit_code: Some(3),
                },
                &ParseContext::default(),
            );
            let blocks = parsed
                .findings
                .iter()
                .any(|f| f.severity == Severity::Error);
            assert!(
                blocks || parsed.outcome == Outcome::Nothing,
                "{} on {stdout:?}: {:?}",
                adapter.name(),
                parsed.outcome
            );
        }
    }
}
