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
    let blocks: Vec<(&str, Vec<&str>)> = std::iter::once(("notable", notable))
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
    blocks
        .iter()
        .map(|(label, lines)| {
            let body = last_bytes(&lines.join("\n"), share);
            if labelled {
                format!("{label}:\n{body}")
            } else {
                body
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
fn notable_lines<'a>(
    stderr: &'a str,
    stdout: &'a str,
    tails: &[(&str, Vec<&str>)],
) -> Vec<&'a str> {
    let mut found: Vec<&str> = Vec::new();
    for line in stderr.lines().chain(stdout.lines()) {
        if found.len() == NOTABLE_LINES {
            break;
        }
        let line = clip(line.trim_end());
        let held = tails.iter().any(|(_, t)| t.contains(&line));
        if !held && !found.contains(&line) && is_notable(line) {
            found.push(line);
        }
    }
    found
}

fn is_notable(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("(node:") || line.contains("DeprecationWarning") {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    ["error", "invalid", "failed", "fail:"].iter().any(|word| {
        lower.match_indices(word).any(|(at, _)| {
            lower
                .get(..at)
                .unwrap_or_default()
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphabetic())
        })
    })
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
            got.starts_with("notable:\n0004-x.md: invalid status \"Acceptedd\"\nstdout:\nok 31\n"),
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
                "notable:\nError 1\nError 2\nError 3\nError 4\nError 5\nstderr:\nError 11\n"
            ),
            "{got}"
        );
        let got = tail("Error 1\nError 1\n", "");
        assert_eq!(got, "Error 1\nError 1");
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
