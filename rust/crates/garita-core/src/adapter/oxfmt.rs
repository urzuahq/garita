use super::{ParseContext, Parsed, ToolFinding, ToolOutput, clip, path_arg, rebase};
use crate::report::Severity;

const HEADER: &str = "Checking formatting...";
const ALL_FORMATTED: &str = "All matched files use the correct format.";
const ISSUES: &str = "Format issues found in above ";
const NO_FILES: [&str; 2] = [
    "Expected at least one target file.",
    "No files found matching the given patterns.",
];

/// Parses `oxfmt --check` or `oxfmt --list-different`.
pub fn parse(output: &ToolOutput<'_>, context: &ParseContext<'_>) -> Parsed {
    let Some(exit) = output.exit_code else {
        return Parsed::killed();
    };
    let said_no_files = |l: &str| NO_FILES.iter().any(|n| l.trim_start().starts_with(n));
    if output
        .stderr
        .lines()
        .chain(output.stdout.lines())
        .any(said_no_files)
    {
        return Parsed::from_findings(Vec::new(), Some(0));
    }
    if output.stdout.lines().any(|l| l.trim() == HEADER) {
        parse_check(output, context, exit)
    } else {
        parse_list(output, context, exit)
    }
}

fn parse_check(output: &ToolOutput<'_>, context: &ParseContext<'_>, exit: i32) -> Parsed {
    let lines: Vec<&str> = output.stdout.lines().map(str::trim).collect();
    let files = lines
        .iter()
        .find_map(|l| finished_count(l))
        .or(context.files);
    if exit == 0 && lines.contains(&ALL_FORMATTED) {
        return Parsed::from_findings(Vec::new(), files);
    }
    let issues = lines.iter().position(|l| l.starts_with(ISSUES));
    let (Some(end), 1) = (issues, exit) else {
        return unreadable(output, exit);
    };
    let start = lines.iter().position(|l| *l == HEADER).map_or(0, |i| i + 1);
    let listed: Vec<&str> = lines
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| strip_duration(l))
        .filter(|l| plausible_path(l))
        .collect();
    let stated = lines
        .get(end)
        .and_then(|l| l.strip_prefix(ISSUES))
        .and_then(leading_number);
    if listed.is_empty() || stated != Some(listed.len() as u64) {
        return Parsed::unrecognized("oxfmt", exit, output);
    }
    Parsed::from_findings(findings(&listed, context.cwd), files)
}

fn parse_list(output: &ToolOutput<'_>, context: &ParseContext<'_>, exit: i32) -> Parsed {
    let mut files = context.files;
    let mut listed = Vec::new();
    let mut ignored = false;
    for line in output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        match finished_count(line) {
            Some(count) => files = Some(count),
            None if plausible_path(line) => listed.push(line),
            None => ignored = true,
        }
    }
    match (exit, listed.is_empty()) {
        (0, true) if !ignored => Parsed::from_findings(Vec::new(), files),
        (1, false) => Parsed::from_findings(findings(&listed, context.cwd), files),
        _ => unreadable(output, exit),
    }
}

/// A sentence or message (as in a task log that merges both streams) is not a file path.
fn plausible_path(line: &str) -> bool {
    !(line.ends_with('.') || line.contains(". ") || line.contains(": ") || line == HEADER)
}

fn findings(listed: &[&str], cwd: &str) -> Vec<ToolFinding> {
    listed
        .iter()
        .map(|path| {
            let file = rebase(cwd, path);
            ToolFinding {
                severity: Severity::Error,
                rule: "oxfmt/unformatted".to_string(),
                fix: Some(format!("oxfmt {}", path_arg(&file))),
                file: Some(file),
                line: None,
                column: None,
                message: "File is not formatted".to_string(),
            }
        })
        .collect()
}

/// Exit 0 with output that should have parsed is a crash; any other exit is the tool's verdict.
fn unreadable(output: &ToolOutput<'_>, exit: i32) -> Parsed {
    if exit == 0 {
        Parsed::crashed(crash_reason(output))
    } else {
        Parsed::unrecognized("oxfmt", exit, output)
    }
}

fn crash_reason(output: &ToolOutput<'_>) -> String {
    let said = output
        .stderr
        .lines()
        .chain(output.stdout.lines())
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != HEADER)
        .next_back();
    match said {
        Some(line) => format!(
            "oxfmt exited 0 with output garita does not recognise: {}",
            clip(line)
        ),
        None => "oxfmt exited 0 without a result".to_string(),
    }
}

/// The count in `Finished in 3ms on 4 files using 20 threads.`
pub(super) fn finished_count(line: &str) -> Option<u64> {
    let rest = line.strip_prefix("Finished in ")?;
    let (_, after) = rest.split_once(" on ")?;
    let (count, unit) = after.split_once(' ')?;
    if !unit.starts_with("file") {
        return None;
    }
    count.parse().ok()
}

fn leading_number(text: &str) -> Option<u64> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// `src/a.ts (0ms)` is `src/a.ts`; a parenthesised part that is not a duration stays.
fn strip_duration(line: &str) -> &str {
    let Some((path, last)) = line.rsplit_once(" (") else {
        return line;
    };
    let Some(inner) = last.strip_suffix(')') else {
        return line;
    };
    let number = inner
        .strip_suffix("ms")
        .or_else(|| inner.strip_suffix('s'))
        .unwrap_or("");
    let is_duration = !number.is_empty() && number.chars().all(|c| c.is_ascii_digit() || c == '.');
    if is_duration { path } else { line }
}
