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

/// oxlint's output format, given as `--format` or `-f`.
fn format_of(args: &[String]) -> Option<&str> {
    option_value(args, "--format").or_else(|| option_value(args, "-f"))
}

fn has_flag(argv: &[String], name: &str) -> bool {
    option_value(argv.get(1..).unwrap_or_default(), name).is_some()
}

impl Adapter {
    /// Appends what this adapter's parser needs when the command does not carry it already.
    /// Through turbo nothing is added: each task runs its package's own script, which may not
    /// be this tool at all, so the parser reads whatever that script prints.
    pub fn complete_argv(self, argv: &mut Vec<String>, via_turbo: bool) -> Result<(), String> {
        if via_turbo {
            let outer = argv.get(1..).unwrap_or_default();
            if self == Adapter::Oxlint && format_of(outer).is_some() {
                return Err(
                    "with `via: turbo`, `--format` before `--` goes to turbo, which rejects it; put it after `--` so it reaches oxlint"
                        .to_string(),
                );
            }
            return Ok(());
        }
        match self {
            Adapter::Oxlint => match format_of(argv.get(1..).unwrap_or_default()) {
                None => insert(argv, &["--format=json"]),
                Some("json") => {}
                Some(other) => {
                    return Err(format!(
                        "the oxlint parser reads `--format=json`, but the command passes `--format {other}`"
                    ));
                }
            },
            Adapter::Tsc if !has_flag(argv, "--pretty") => {
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
    fn oxlint_format_given_as_dash_f_is_read_too() {
        for words in [
            &["oxlint", "-f", "stylish", "."][..],
            &["oxlint", "-f=unix", "."],
        ] {
            let err = completed(Adapter::Oxlint, words, false).unwrap_err();
            assert!(err.contains("--format"), "{err}");
        }
        assert_eq!(
            completed(Adapter::Oxlint, &["oxlint", "-f", "json", "."], false),
            Ok(argv(&["oxlint", "-f", "json", "."]))
        );
        assert!(completed(Adapter::Oxlint, &["turbo", "lint", "-f", "json"], true).is_err());
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
    fn via_turbo_the_command_gets_only_turbos_own_flags() {
        let turbo = |adapter: Adapter, words: &[&str]| {
            let mut a = argv(words);
            adapter.complete_argv(&mut a, true)?;
            complete_turbo_argv(&mut a);
            Ok::<_, String>(a)
        };
        assert_eq!(
            turbo(
                Adapter::Oxlint,
                &["pnpm", "turbo", "lint:check", "--output-logs=errors-only"]
            ),
            Ok(argv(&[
                "pnpm",
                "turbo",
                "lint:check",
                "--output-logs=errors-only",
                "--summarize",
                "--continue",
            ]))
        );
        assert_eq!(
            turbo(
                Adapter::Oxlint,
                &["turbo", "lint:check", "--", "--quiet", "--", "extra"]
            ),
            Ok(argv(&[
                "turbo",
                "lint:check",
                "--summarize",
                "--continue",
                "--",
                "--quiet",
                "--",
                "extra",
            ]))
        );
        assert_eq!(
            turbo(
                Adapter::Oxlint,
                &["turbo", "lint", "--", "--format=stylish"]
            ),
            Ok(argv(&[
                "turbo",
                "lint",
                "--summarize",
                "--continue",
                "--",
                "--format=stylish"
            ]))
        );
        let err = turbo(Adapter::Oxlint, &["turbo", "lint:check", "--format=json"]).unwrap_err();
        assert!(err.contains("after `--`"), "{err}");
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
