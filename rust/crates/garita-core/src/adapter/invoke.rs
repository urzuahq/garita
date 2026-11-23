//! The flags an adapter's parser needs on the command line (RFC-2 §2).

use super::Adapter;

/// Inserts `flags` before a literal `--`, which ends a tool's options, else at the end.
fn insert(argv: &mut Vec<String>, flags: &[&str]) {
    let at = argv
        .iter()
        .skip(1)
        .position(|a| a == "--")
        .map_or(argv.len(), |i| i + 1);
    for (offset, flag) in flags.iter().enumerate() {
        argv.insert(at + offset, (*flag).to_string());
    }
}

/// The arguments after the first `--`, which turbo hands to each task's command.
fn inner(argv: &[String]) -> &[String] {
    let skip = argv
        .iter()
        .skip(1)
        .position(|a| a == "--")
        .map_or(argv.len(), |i| i + 2);
    argv.get(skip..).unwrap_or_default()
}

/// The value of an option given as `--name=value` or `--name value` among `args`, up to any `--`.
fn option_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    let mut args = args.iter().take_while(|a| *a != "--");
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix(&prefix) {
            return Some(value);
        }
        if arg == name {
            return Some(args.next().map_or("", String::as_str));
        }
    }
    None
}

fn has_flag(argv: &[String], name: &str) -> bool {
    option_value(argv.get(1..).unwrap_or_default(), name).is_some()
}

/// Adds `flags` for the tool turbo runs right after the first `--`, else after an appended one.
fn insert_inner(argv: &mut Vec<String>, flags: &[&str]) {
    let at = match argv.iter().skip(1).position(|a| a == "--") {
        Some(i) => i + 2,
        None => {
            argv.push("--".to_string());
            argv.len()
        }
    };
    for (offset, flag) in flags.iter().enumerate() {
        argv.insert(at + offset, (*flag).to_string());
    }
}

impl Adapter {
    /// Appends what this adapter's parser needs when the command does not carry it already;
    /// through turbo, those flags belong to the inner tool, after `--`.
    pub fn complete_argv(self, argv: &mut Vec<String>, via_turbo: bool) -> Result<(), String> {
        match self {
            Adapter::Oxlint => {
                let outer = argv.get(1..).unwrap_or_default();
                if via_turbo && option_value(outer, "--format").is_some() {
                    return Err(
                        "with `via: turbo`, `--format` before `--` goes to turbo, which rejects it; put it after `--` so it reaches oxlint"
                            .to_string(),
                    );
                }
                let args = if via_turbo {
                    inner(argv)
                } else {
                    argv.get(1..).unwrap_or_default()
                };
                match option_value(args, "--format") {
                    None if via_turbo => insert_inner(argv, &["--format=json"]),
                    None => insert(argv, &["--format=json"]),
                    Some("json") => {}
                    Some(other) => {
                        return Err(format!(
                            "the oxlint parser reads `--format=json`, but the command passes `--format {other}`"
                        ));
                    }
                }
            }
            Adapter::Tsc if !via_turbo && !has_flag(argv, "--pretty") => {
                insert(argv, &["--pretty", "false"]);
            }
            Adapter::Tsc | Adapter::Oxfmt | Adapter::ExitCode => {}
        }
        Ok(())
    }
}

/// Appends `--summarize` and `--continue` to a turbo command when absent (RFC-2 §5).
pub fn complete_turbo_argv(argv: &mut Vec<String>) {
    for flag in ["--summarize", "--continue"] {
        let present = argv
            .iter()
            .skip(1)
            .take_while(|a| *a != "--")
            .any(|a| a == flag || a.starts_with(&format!("{flag}=")));
        if !present {
            insert(argv, &[flag]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    fn completed(adapter: Adapter, words: &[&str], via_turbo: bool) -> Result<Vec<String>, String> {
        let mut a = argv(words);
        adapter.complete_argv(&mut a, via_turbo).map(|()| a)
    }

    #[test]
    fn oxlint_gets_json_unless_it_has_it() {
        assert_eq!(
            completed(Adapter::Oxlint, &["oxlint", "a.ts"], false),
            Ok(argv(&["oxlint", "a.ts", "--format=json"]))
        );
        assert_eq!(
            completed(
                Adapter::Oxlint,
                &["oxlint", "--format", "json", "a.ts"],
                false
            ),
            Ok(argv(&["oxlint", "--format", "json", "a.ts"]))
        );
        assert_eq!(
            completed(Adapter::Oxlint, &["oxlint", "--format=json"], false),
            Ok(argv(&["oxlint", "--format=json"]))
        );
        assert_eq!(
            completed(Adapter::Oxlint, &["oxlint", "--", "-a.ts"], false),
            Ok(argv(&["oxlint", "--format=json", "--", "-a.ts"]))
        );
    }

    #[test]
    fn oxlint_with_another_format_is_an_error() {
        let err = completed(Adapter::Oxlint, &["oxlint", "--format=stylish"], false).unwrap_err();
        assert!(err.contains("--format stylish"), "{err}");
    }

    #[test]
    fn tsc_gets_plain_output_except_behind_turbo() {
        assert_eq!(
            completed(Adapter::Tsc, &["tsc", "--noEmit"], false),
            Ok(argv(&["tsc", "--noEmit", "--pretty", "false"]))
        );
        assert_eq!(
            completed(Adapter::Tsc, &["tsc", "--pretty", "false"], false),
            Ok(argv(&["tsc", "--pretty", "false"]))
        );
        assert_eq!(
            completed(Adapter::Tsc, &["turbo", "type-check"], true),
            Ok(argv(&["turbo", "type-check"]))
        );
    }

    #[test]
    fn oxfmt_and_exit_code_are_left_alone() {
        for adapter in [Adapter::Oxfmt, Adapter::ExitCode] {
            assert_eq!(
                completed(adapter, &["t", "x"], false),
                Ok(argv(&["t", "x"]))
            );
        }
    }

    #[test]
    fn oxlint_via_turbo_gets_json_after_the_separator() {
        let turbo = |words: &[&str]| {
            let mut a = argv(words);
            Adapter::Oxlint.complete_argv(&mut a, true)?;
            complete_turbo_argv(&mut a);
            Ok::<_, String>(a)
        };
        assert_eq!(
            turbo(&["pnpm", "turbo", "lint:check", "--output-logs=errors-only"]),
            Ok(argv(&[
                "pnpm",
                "turbo",
                "lint:check",
                "--output-logs=errors-only",
                "--summarize",
                "--continue",
                "--",
                "--format=json",
            ]))
        );
        assert_eq!(
            turbo(&["turbo", "lint", "--", "--quiet"]),
            Ok(argv(&[
                "turbo",
                "lint",
                "--summarize",
                "--continue",
                "--",
                "--format=json",
                "--quiet",
            ]))
        );
        assert_eq!(
            turbo(&["turbo", "lint", "--", "--format", "json"]),
            Ok(argv(&[
                "turbo",
                "lint",
                "--summarize",
                "--continue",
                "--",
                "--format",
                "json",
            ]))
        );
        assert_eq!(
            turbo(&["turbo", "lint:check", "--", "--quiet", "--", "extra"]),
            Ok(argv(&[
                "turbo",
                "lint:check",
                "--summarize",
                "--continue",
                "--",
                "--format=json",
                "--quiet",
                "--",
                "extra",
            ]))
        );
        let err = turbo(&["turbo", "lint:check", "--format=json"]).unwrap_err();
        assert!(err.contains("after `--`"), "{err}");
        let err = turbo(&["turbo", "lint", "--", "--format=stylish"]).unwrap_err();
        assert!(err.contains("--format stylish"), "{err}");
    }

    #[test]
    fn turbo_gets_summarize_and_continue_once() {
        let mut a = argv(&["turbo", "run", "t", "--continue", "--", "x"]);
        complete_turbo_argv(&mut a);
        assert_eq!(
            a,
            argv(&["turbo", "run", "t", "--continue", "--summarize", "--", "x"])
        );
        complete_turbo_argv(&mut a);
        assert_eq!(a.iter().filter(|w| *w == "--summarize").count(), 1);
    }
}
