use super::{Outcome, ParseContext, Parsed, ToolFinding, ToolOutput, clip, examined_files};
use crate::report::Severity;

const STREAM_LINES: usize = 10;
const TAIL_BYTES: usize = 4000;
const NOTABLE_LINES: usize = 5;

/// Reads only the exit code, keeping the output's last lines as the finding's message.
pub fn parse(output: &ToolOutput<'_>, context: &ParseContext<'_>) -> Parsed {
    let Some(exit) = output.exit_code else {
        return Parsed::killed();
    };
    if exit == 0 {
        return Parsed::from_findings(Vec::new(), context.files);
    }
    let tail = tail(output.stderr, output.stdout);
    let message = if tail.is_empty() {
        format!("exited with code {exit} and printed nothing")
    } else {
        format!("exited with code {exit}:\n{tail}")
    };
    Parsed {
        outcome: Outcome::Findings,
        findings: vec![ToolFinding {
            severity: Severity::Error,
            rule: "exit-code/failed".to_string(),
            file: None,
            line: None,
            column: None,
            message,
            fix: None,
        }],
        examined: examined_files(context.files),
    }
}

/// Up to [`NOTABLE_LINES`] lines from anywhere in the output that look like a reason, then the
/// last non-blank lines of stderr and of stdout; each block gets at most [`STREAM_LINES`] and an
/// equal share of [`TAIL_BYTES`], and blocks are labelled when there is more than one.
pub fn tail(stderr: &str, stdout: &str) -> String {
    let tails = [
        ("stderr", last_lines(stderr)),
        ("stdout", last_lines(stdout)),
    ];
    let notable = notable_lines(stderr, stdout, &tails);
    let (from_stderr, from_stdout): (Vec<_>, Vec<_>) = notable.into_iter().partition(|(e, _)| *e);
    let blocks: Vec<(&str, Vec<&str>)> = [
        ("stderr notable", second(from_stderr)),
        ("stdout notable", second(from_stdout)),
    ]
    .into_iter()
    .chain(tails)
    .filter(|(_, lines)| !lines.is_empty())
    .collect();
    let labelled = blocks.len() > 1;
    let labels: usize = if labelled {
        blocks.iter().map(|(label, _)| label.len() + 3).sum()
    } else {
        0
    };
    let share = TAIL_BYTES.saturating_sub(labels) / blocks.len().max(1);
    let joined = blocks
        .iter()
        .map(|(label, lines)| {
            let body = last_bytes(&lines.join("\n"), share);
            if labelled {
                format!("{label}:\n{body}")
            } else {
                body
            }
        })
        .collect::<Vec<_>>();
    sanitize(&joined.join("\n"))
}

fn last_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text
        .lines()
        .rev()
        .map(|l| clip(l.trim_end()))
        .filter(|l| !l.trim().is_empty())
        .take(STREAM_LINES)
        .collect();
    lines.reverse();
    lines
}

/// Lines from both streams, in order, that name an error, skipping Node's deprecation noise and
/// lines the tails already hold.
fn second(pairs: Vec<(bool, &str)>) -> Vec<&str> {
    pairs.into_iter().map(|(_, line)| line).collect()
}

fn notable_lines<'a>(
    stderr: &'a str,
    stdout: &'a str,
    tails: &[(&str, Vec<&str>)],
) -> Vec<(bool, &'a str)> {
    let mut found: Vec<(bool, &str)> = Vec::new();
    let lines = stderr
        .lines()
        .map(|l| (true, l))
        .chain(stdout.lines().map(|l| (false, l)));
    for (from_stderr, line) in lines {
        if found.len() == NOTABLE_LINES {
            break;
        }
        let line = clip(line.trim_end());
        let held = tails.iter().any(|(_, t)| t.contains(&line));
        if !held && !found.iter().any(|(_, l)| *l == line) && is_notable(line) {
            found.push((from_stderr, line));
        }
    }
    found
}

const KEYWORDS: [&str; 10] = [
    "error", "errors", "invalid", "failed", "failure", "failing", "fatal", "panic", "panicked",
    "fail",
];
const NEGATED_BEFORE: [&str; 3] = ["0", "no", "without"];

/// Whether `line` names a failure: a whole-word keyword that is not negated or counted as zero,
/// a `SomethingError:` token, or a failure marker at its start.
pub fn is_notable(line: &str) -> bool {
    if is_noise(line) {
        return false;
    }
    let trimmed = line.trim_start();
    let marker = ["FAIL ", "× ", "✗ ", "x "].iter().any(|m| {
        trimmed
            .strip_prefix(m)
            .is_some_and(|rest| !rest.trim().is_empty())
    });
    let words = tokens(line);
    marker || (0..words.len()).any(|i| keyword_at(&words, i))
}

/// Words of letters and digits, with the text that follows each.
fn tokens(line: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (at, c) in line
        .char_indices()
        .chain(std::iter::once((line.len(), ' ')))
    {
        match (start, c.is_alphanumeric() || c == '_') {
            (None, true) => start = Some(at),
            (Some(from), false) => {
                out.push((
                    line.get(from..at).unwrap_or_default(),
                    line.get(at..).unwrap_or_default(),
                ));
                start = None;
            }
            _ => {}
        }
    }
    out
}

fn keyword_at(all: &[(&str, &str)], i: usize) -> bool {
    let Some((word, after)) = all.get(i) else {
        return false;
    };
    if word.len() > 5 && word.ends_with("Error") && after.starts_with(':') {
        return true;
    }
    let lower = word.to_ascii_lowercase();
    if !KEYWORDS.contains(&lower.as_str()) {
        return false;
    }
    if lower == "fail" && !(after.starts_with(':') || *word == "FAIL") {
        return false;
    }
    let before = i
        .checked_sub(1)
        .and_then(|j| all.get(j))
        .map(|(w, _)| w.to_ascii_lowercase());
    let negated_before = before
        .as_deref()
        .is_some_and(|b| NEGATED_BEFORE.contains(&b));
    let zero_after = after.strip_prefix(':').is_some_and(|rest| {
        rest.trim_start()
            .split(|c: char| !c.is_ascii_digit())
            .next()
            == Some("0")
    });
    !negated_before && !zero_after
}

/// Lines that never explain a failure: runtime warnings, banners, and source excerpts.
pub fn is_noise(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.is_empty() && !trimmed.chars().any(char::is_alphanumeric) {
        return true;
    }
    if trimmed.starts_with("(node:")
        || line.contains("DeprecationWarning")
        || trimmed.starts_with("(Use `node --trace-")
    {
        return true;
    }
    if [",-[", "`----", "·", "╭", "╰", "│"]
        .iter()
        .any(|p| trimmed.starts_with(p))
    {
        return true;
    }
    let digits = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
    if digits.len() < trimmed.len() && digits.trim_start().starts_with(['|', '│']) {
        return true;
    }
    let mut words = trimmed.split_whitespace();
    matches!(
        (words.next(), words.next(), words.next()),
        (Some(name), Some(version), None)
            if name.chars().all(|c| c.is_alphanumeric() || "-_@/".contains(c))
                && version.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit())
                && version.contains('.')
    )
}

/// Control characters other than tab and newline become `?`, so output cannot drive a terminal.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\t' && c != '\n' {
                '?'
            } else {
                c
            }
        })
        .collect()
}

fn last_bytes(text: &str, limit: usize) -> String {
    let mut start = text.len().saturating_sub(limit);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text.get(start..).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(name: &str, n: usize) -> String {
        (1..=n).map(|i| format!("{name} {i}\n")).collect()
    }

    #[test]
    fn each_stream_keeps_its_own_last_lines_under_a_label() {
        let stderr = "--fail-on-drift set: failing\n";
        let got = tail(stderr, &numbered("json", 50));
        let want = format!(
            "stderr:\n--fail-on-drift set: failing\nstdout:\n{}",
            numbered("json", 50)
                .lines()
                .skip(40)
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert_eq!(got, want);
        let got = tail(&numbered("warn", 30), "error: the real one\n");
        assert!(got.starts_with("stderr:\nwarn 21\n"), "{got}");
        assert!(got.ends_with("stdout:\nerror: the real one"), "{got}");
    }

    #[test]
    fn an_early_reason_line_is_kept_as_notable() {
        let mut stdout = "0004-x.md: invalid status \"Acceptedd\"\n".to_string();
        stdout.push_str("(node:1) [DEP0169] DeprecationWarning: error-ish noise\n");
        stdout.push_str("noerrors here\n");
        stdout.push_str(&numbered("ok", 40));
        let got = tail("", &stdout);
        assert!(
            got.starts_with(
                "stdout notable:\n0004-x.md: invalid status \"Acceptedd\"\nstdout:\nok 31\n"
            ),
            "{got}"
        );
        assert!(
            !got.contains("Deprecation") && !got.contains("noerrors"),
            "{got}"
        );
    }

    #[test]
    fn notable_lines_are_capped_and_not_repeated_from_the_tails() {
        let stderr: String = (1..=20).map(|i| format!("Error {i}\n")).collect();
        let got = tail(&stderr, &numbered("x", 30));
        assert!(
            got.starts_with(
                "stderr notable:\nError 1\nError 2\nError 3\nError 4\nError 5\nstderr:\nError 11\n"
            ),
            "{got}"
        );
        let got = tail("Error 1\nError 1\n", "");
        assert_eq!(got, "Error 1\nError 1");
    }

    #[test]
    fn keywords_are_whole_words_and_not_negated() {
        for line in [
            "error: x",
            "src/a.ts(1,2): error TS2322: bad",
            "Error: boom",
            "[error] it broke",
            "2 errors",
            "build failed",
            "test failure in a",
            "1 failing",
            "FAIL: thing",
            "thread 'main' panicked at src/x.rs",
            "panic: runtime error",
            "FATAL: migration 42 out of order",
            "AssertionError: expected 1",
            "TypeError: x is undefined",
            "x eslint(no-debugger): `debugger` statement is not allowed",
            " FAIL  src/a.test.ts > ab fails",
            "× parse failed",
            "✗ step 3",
            "0004-x.md: invalid status \"Acceptedd\"",
        ] {
            assert!(is_notable(line), "{line}");
        }
        for line in [
            "Found 0 errors.",
            "pkg-a: Found 0 errors.",
            "no error in pkg 2",
            "no errors",
            "errors: 0",
            "error: 0",
            "0 failed, 3 passed",
            "0 failures",
            "no failures",
            "finished without error",
            "src/errorHandler.ts",
            "failedCount=3",
            "the word failure_rate",
            "fail without colon",
            "x",
            "(node:3974505) [DEP0169] DeprecationWarning: url.parse() error",
            "(Use `node --trace-deprecation ...` to show where the warning was created)",
            "83 | | (error: unknown) => {",
            "  12 │ throw new Error('x')",
            "   ,-[src/error.ts:2:3]",
            "   `---- error",
            "  · error here",
            "╭─[error.ts]",
            "╰── error",
            "│ error",
        ] {
            assert!(!is_notable(line), "{line}");
        }
    }

    #[test]
    fn noise_covers_banners_and_excerpts_only() {
        for line in [
            "turbo 2.5.6",
            "pnpm v9.1.0",
            "83 | code",
            "(node:1) x",
            "   :   ^^^^^^^^^",
            "  ----  ",
            "─┬─",
        ] {
            assert!(is_noise(line), "{line}");
        }
        for line in ["turbo failed", "step 2 of 3", "a b c 1.0", "FATAL: x"] {
            assert!(!is_noise(line), "{line}");
        }
    }

    #[test]
    fn control_characters_are_replaced() {
        assert_eq!(sanitize("a\u{1b}[2Jb\tc\nd\u{7}"), "a?[2Jb\tc\nd?");
        assert_eq!(tail("\u{0}bin\u{8}ary\n", ""), "?bin?ary");
    }

    #[test]
    fn one_stream_is_unlabelled_and_an_empty_one_adds_nothing() {
        assert_eq!(tail("", "a\n\nb\n"), "a\nb");
        assert_eq!(tail("boom\n", "  \n"), "boom");
        assert_eq!(tail("", ""), "");
    }

    #[test]
    fn the_tail_stays_within_the_cap_and_clips_lines() {
        let long = format!("{}\n", "x".repeat(5000)).repeat(20);
        let got = tail(&long, &long);
        assert!(got.len() <= TAIL_BYTES, "{}", got.len());
        assert!(got.lines().all(|l| l.len() <= 300));
    }
}
