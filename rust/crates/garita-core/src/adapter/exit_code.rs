use super::{Outcome, ParseContext, Parsed, ToolFinding, ToolOutput, clip, examined_files};
use crate::report::Severity;

const NOTABLE_LINES: usize = 5;
const LAST_LINES: usize = 10;

/// Reads only the exit code; the output stays on the check, with its likely reason picked out.
pub fn parse(output: &ToolOutput<'_>, context: &ParseContext<'_>) -> Parsed {
    let Some(exit) = output.exit_code else {
        return Parsed::killed();
    };
    if exit == 0 {
        return Parsed::from_findings(Vec::new(), context.files);
    }
    let message = if printed_nothing(output.stderr, output.stdout) {
        format!("exited with code {exit} and printed nothing")
    } else {
        format!("exited with code {exit}")
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
        notable: notable(output.stderr, output.stdout),
    }
}

/// Whether neither stream holds a non-blank line.
pub fn printed_nothing(stderr: &str, stdout: &str) -> bool {
    stderr.trim().is_empty() && stdout.trim().is_empty()
}

/// The last [`LAST_LINES`] non-blank lines of `text`, clipped and with control characters replaced.
pub fn last_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .rev()
        .map(|l| clip(l.trim()))
        .filter(|l| !l.is_empty())
        .take(LAST_LINES)
        .map(sanitize)
        .collect();
    lines.reverse();
    lines
}

/// Up to [`NOTABLE_LINES`] distinct lines, stderr first, that look like the reason a command
/// failed, skipping runtime noise; each is clipped and has its control characters replaced.
pub fn notable(stderr: &str, stdout: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in stderr.lines().chain(stdout.lines()) {
        if found.len() == NOTABLE_LINES {
            break;
        }
        let line = sanitize(clip(line.trim()));
        if is_notable(&line) && !found.contains(&line) {
            found.push(line);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notable_lines_come_from_anywhere_in_the_output() {
        let mut stdout = "0004-x.md: invalid status \"Acceptedd\"\n".to_string();
        stdout.push_str("(node:1) [DEP0169] DeprecationWarning: error-ish noise\n");
        stdout.push_str("noerrors here\n");
        stdout.push_str(&"ok\n".repeat(40));
        assert_eq!(
            notable("", &stdout),
            ["0004-x.md: invalid status \"Acceptedd\""]
        );
    }

    #[test]
    fn notable_lines_are_capped_distinct_and_stderr_first() {
        let stderr: String = (1..=20).map(|i| format!("Error {i}\n")).collect();
        assert_eq!(
            notable(&stderr, "error: last\n"),
            ["Error 1", "Error 2", "Error 3", "Error 4", "Error 5"]
        );
        assert_eq!(
            notable("Error 1\nError 1\n", "error: b\n"),
            ["Error 1", "error: b"]
        );
        assert!(notable("", "").is_empty());
    }

    #[test]
    fn a_failure_message_is_one_sentence() {
        let context = ParseContext::default();
        let output = ToolOutput {
            stdout: "line one\nerror: two\n",
            stderr: "",
            exit_code: Some(3),
        };
        let parsed = parse(&output, &context);
        assert_eq!(parsed.findings[0].message, "exited with code 3");
        assert_eq!(parsed.notable, ["error: two"]);
        let silent = ToolOutput {
            stdout: " \n",
            ..output
        };
        assert_eq!(
            parse(&silent, &context).findings[0].message,
            "exited with code 3 and printed nothing"
        );
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
    fn last_lines_skip_blanks_and_keep_order() {
        let text: String = (1..=30).map(|i| format!("line {i}\n\n")).collect();
        let got = last_lines(&text);
        assert_eq!(got.len(), 10);
        assert_eq!((got[0].as_str(), got[9].as_str()), ("line 21", "line 30"));
    }

    #[test]
    fn a_line_indented_past_the_cap_keeps_its_text() {
        let text = format!("first\n{}boom\n", " ".repeat(crate::adapter::LINE_CAP + 1));
        assert_eq!(last_lines(&text), ["first", "boom"]);
    }

    #[test]
    fn control_characters_are_replaced() {
        assert_eq!(sanitize("a\u{1b}[2Jb\tc\nd\u{7}"), "a?[2Jb\tc\nd?");
        assert_eq!(
            notable("\u{0}error: bin\u{8}ary\n", ""),
            ["?error: bin?ary"]
        );
    }

    #[test]
    fn notable_lines_are_clipped() {
        let long = format!("error: {}\n", "x".repeat(5000));
        assert!(notable(&long, "").iter().all(|l| l.len() <= 300));
    }
}
