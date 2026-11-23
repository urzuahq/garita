use super::{ParseContext, Parsed, ToolFinding, ToolOutput, nonzero, rebase};
use crate::report::Severity;

/// Parses `--pretty false` diagnostics from TypeScript 5.x and the native compiler.
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
        return Parsed::crashed(crash_reason(output, exit));
    }
    Parsed::from_findings(findings, context.files)
}

fn crash_reason(output: &ToolOutput<'_>, exit: i32) -> String {
    let said = output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .map(str::trim)
        .find(|l| !l.is_empty());
    match said {
        Some(line) => format!("tsc exited {exit} without a diagnostic: {line}"),
        None => format!("tsc exited {exit} without a diagnostic"),
    }
}

/// `path(line,col): error TSnnnn: message`, or the file-less `error TSnnnn: message`.
fn diagnostic(line: &str, cwd: &str) -> Option<ToolFinding> {
    let line = line.trim_end();
    if let Some(head) = head(line) {
        return Some(head.finding(None, None, None));
    }
    let mut search = 0;
    while let Some(found) = line.get(search..)?.find("): ") {
        let close = search + found;
        let rest = line.get(close + 3..)?;
        if let (Some((path, row, col)), Some(head)) = (location(line.get(..close)?), head(rest)) {
            return Some(head.finding(Some(rebase(cwd, path)), nonzero(row), nonzero(col)));
        }
        search = close + 3;
    }
    None
}

/// Splits `path(12,5` into the path and its position.
fn location(text: &str) -> Option<(&str, u64, u64)> {
    let (path, position) = text.rsplit_once('(')?;
    let (row, col) = position.split_once(',')?;
    if path.is_empty() || !all_digits(row) || !all_digits(col) {
        return None;
    }
    Some((path, row.parse().ok()?, col.parse().ok()?))
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

/// `error TS2322: message`.
fn head(text: &str) -> Option<Head<'_>> {
    let (word, rest) = text.split_once(' ')?;
    let severity = match word {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "message" | "suggestion" => Severity::Info,
        _ => return None,
    };
    let (code, message) = rest.strip_prefix("TS")?.split_once(": ")?;
    if !all_digits(code) {
        return None;
    }
    Some(Head {
        severity,
        code,
        message,
    })
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
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
