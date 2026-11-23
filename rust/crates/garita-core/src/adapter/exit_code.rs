use super::{Outcome, ParseContext, Parsed, ToolFinding, ToolOutput, clip, examined_files};
use crate::report::Severity;

const STREAM_LINES: usize = 10;
const TAIL_BYTES: usize = 4000;

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

/// The last non-blank lines of stderr and of stdout, each stream given its own
/// [`STREAM_LINES`] and an equal share of [`TAIL_BYTES`], labelled when both have any.
pub fn tail(stderr: &str, stdout: &str) -> String {
    let streams: Vec<(&str, Vec<&str>)> = [("stderr", stderr), ("stdout", stdout)]
        .into_iter()
        .map(|(label, text)| (label, last_lines(text)))
        .filter(|(_, lines)| !lines.is_empty())
        .collect();
    let labelled = streams.len() > 1;
    let labels = if labelled {
        "stderr:\nstdout:\n\n".len()
    } else {
        0
    };
    let share = (TAIL_BYTES - labels) / streams.len().max(1);
    streams
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
    let lines: Vec<&str> = text
        .lines()
        .map(|l| clip(l.trim_end()))
        .filter(|l| !l.trim().is_empty())
        .collect();
    let keep = lines.len().saturating_sub(STREAM_LINES);
    lines.get(keep..).unwrap_or_default().to_vec()
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
