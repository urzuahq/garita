use super::{ParseContext, Parsed, ToolFinding, ToolOutput, nonzero, rebase};
use crate::report::Severity;

/// Parses diagnostics from TypeScript 5.x and the native compiler, plain or pretty.
pub fn parse(output: &ToolOutput<'_>, context: &ParseContext<'_>) -> Parsed {
    let Some(exit) = output.exit_code else {
        return Parsed::killed();
    };
    let mut findings: Vec<ToolFinding> = Vec::new();
    let mut open = false;
    for line in output.stdout.lines().chain(output.stderr.lines()) {
        if let Some(finding) = diagnostic(line, context.cwd) {
            findings.push(finding);
            open = true;
        } else if open && line.starts_with([' ', '\t']) && !line.trim().is_empty() {
            if let Some(last) = findings.last_mut() {
                last.message.push('\n');
                last.message.push_str(line.trim_end());
            }
        } else {
            open = false;
        }
    }
    if findings.is_empty() && exit != 0 {
        return Parsed::unrecognized("tsc", exit, output);
    }
    Parsed::from_findings(findings, context.files)
}

const WORDS: [(&str, Severity); 4] = [
    ("error ", Severity::Error),
    ("warning ", Severity::Warning),
    ("message ", Severity::Info),
    ("suggestion ", Severity::Info),
];

/// One diagnostic line in any of its forms:
/// `path(line,col): error TSnnnn: message` (`--pretty false`),
/// `path:line:col - error TSnnnn: message` (pretty), or `error TSnnnn: message` (no file).
/// Each `TS` in the line is tried once and checked only against the text next to it, so a
/// line is matched in linear time.
fn diagnostic(line: &str, cwd: &str) -> Option<ToolFinding> {
    let line = line.trim_end();
    for (at, _) in line.match_indices("TS") {
        let before = line.get(..at)?;
        let Some((word, severity)) = WORDS.iter().find(|(w, _)| before.ends_with(w)) else {
            continue;
        };
        let Some((code, message)) = code_and_message(line.get(at + 2..)?) else {
            continue;
        };
        let prefix = before.get(..before.len() - word.len())?;
        let head = Head {
            severity: *severity,
            code,
            message,
        };
        if prefix.is_empty() {
            return Some(head.finding(None, None, None));
        }
        if let Some((path, row, col)) = location(prefix) {
            return Some(head.finding(Some(rebase(cwd, path)), nonzero(row), nonzero(col)));
        }
    }
    None
}

/// `2322: message` after `TS`; only the digits and the `: ` after them are read.
fn code_and_message(text: &str) -> Option<(&str, &str)> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let code = text.get(..digits)?;
    let message = text.get(digits..)?.strip_prefix(": ")?;
    (!code.is_empty()).then_some((code, message))
}

/// `path(12,5): ` or `path:12:5 - `, read backwards over the digits only.
fn location(prefix: &str) -> Option<(&str, u64, u64)> {
    if let Some(rest) = prefix.strip_suffix("): ") {
        let (rest, col) = trailing_number(rest)?;
        let (rest, row) = trailing_number(rest.strip_suffix(',')?)?;
        let path = rest.strip_suffix('(')?;
        return (!path.is_empty()).then_some((path, row, col));
    }
    let rest = prefix.strip_suffix(" - ")?;
    let (rest, col) = trailing_number(rest)?;
    let (rest, row) = trailing_number(rest.strip_suffix(':')?)?;
    let path = rest.strip_suffix(':')?;
    (!path.is_empty()).then_some((path, row, col))
}

fn trailing_number(text: &str) -> Option<(&str, u64)> {
    let digits = text.bytes().rev().take_while(u8::is_ascii_digit).count();
    let split = text.len() - digits;
    let number = text.get(split..)?;
    if number.is_empty() || digits > 19 {
        return None;
    }
    Some((text.get(..split)?, number.parse().ok()?))
}

struct Head<'a> {
    severity: Severity,
    code: &'a str,
    message: &'a str,
}

impl Head<'_> {
    fn finding(
        &self,
        file: Option<String>,
        line: Option<std::num::NonZeroU64>,
        column: Option<std::num::NonZeroU64>,
    ) -> ToolFinding {
        ToolFinding {
            severity: self.severity,
            rule: format!("tsc/TS{}", self.code),
            file,
            line,
            column,
            message: self.message.to_string(),
            fix: None,
        }
    }
}

/// Counts the files in `--listFilesOnly` output, leaving out the compiler's own library: a
/// `lib.*.d.ts` file in a directory named `lib` under `node_modules`.
pub fn count_project_files(list_files_only: &str) -> u64 {
    let count = list_files_only
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !is_compiler_lib(l))
        .count();
    u64::try_from(count).unwrap_or(u64::MAX)
}

fn is_compiler_lib(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let mut segments = path.rsplit('/');
    let name = segments.next().unwrap_or("");
    let parent = segments.next().unwrap_or("");
    name.starts_with("lib.")
        && name.ends_with(".d.ts")
        && parent == "lib"
        && path.contains("/node_modules/")
}
