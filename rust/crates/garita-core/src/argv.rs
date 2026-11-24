//! A check's `run` string as arguments (RFC-1 §2): POSIX quoting, no expansion.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArgvError {
    #[error("the command is empty")]
    Empty,
    #[error("a single quote is not closed")]
    UnterminatedSingleQuote,
    #[error("a double quote is not closed")]
    UnterminatedDoubleQuote,
    #[error("the command ends in a backslash that escapes nothing")]
    TrailingBackslash,
    #[error(
        "`{word}` embeds {placeholder} inside a larger word; a placeholder must be an argument on its own"
    )]
    EmbeddedPlaceholder {
        word: String,
        placeholder: Placeholder,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placeholder {
    Files,
    Affected,
}

impl Placeholder {
    pub const ALL: [Placeholder; 2] = [Placeholder::Files, Placeholder::Affected];

    pub fn as_str(self) -> &'static str {
        match self {
            Placeholder::Files => "{files}",
            Placeholder::Affected => "{affected}",
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == word)
    }
}

impl std::fmt::Display for Placeholder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Literal(String),
    Placeholder(Placeholder),
}

/// A `run` string split into arguments, with its placeholders identified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    args: Vec<Arg>,
}

impl Command {
    pub fn parse(run: &str) -> Result<Self, ArgvError> {
        let words = split(run)?;
        if words.is_empty() {
            return Err(ArgvError::Empty);
        }
        let args = words
            .into_iter()
            .map(classify)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { args })
    }

    pub fn args(&self) -> &[Arg] {
        &self.args
    }

    /// The placeholders the command carries, in order of first appearance.
    pub fn placeholders(&self) -> Vec<Placeholder> {
        let mut found = Vec::new();
        for arg in &self.args {
            if let Arg::Placeholder(p) = arg {
                if !found.contains(p) {
                    found.push(*p);
                }
            }
        }
        found
    }

    /// A command with no placeholder always runs, whatever its scope matched.
    pub fn has_placeholder(&self) -> bool {
        self.args.iter().any(|a| matches!(a, Arg::Placeholder(_)))
    }

    pub fn expand(&self, files: &[String], affected: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for arg in &self.args {
            match arg {
                Arg::Literal(word) => out.push(word.clone()),
                Arg::Placeholder(Placeholder::Files) => out.extend(files.iter().cloned()),
                Arg::Placeholder(Placeholder::Affected) => out.extend(affected.iter().cloned()),
            }
        }
        out
    }
}

fn classify(word: String) -> Result<Arg, ArgvError> {
    if let Some(p) = Placeholder::from_word(&word) {
        return Ok(Arg::Placeholder(p));
    }
    match Placeholder::ALL
        .into_iter()
        .find(|p| word.contains(p.as_str()))
    {
        Some(placeholder) => Err(ArgvError::EmbeddedPlaceholder { word, placeholder }),
        None => Ok(Arg::Literal(word)),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Between,
    Unquoted,
    Single,
    Double,
}

/// Splits by POSIX shell quoting rules without expanding variables, globs or operators.
pub fn split(input: &str) -> Result<Vec<String>, ArgvError> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut state = State::Between;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        state = match state {
            State::Between | State::Unquoted => {
                step_unquoted(c, state, &mut chars, &mut word, &mut words)?
            }
            State::Single if c == '\'' => State::Unquoted,
            State::Single => {
                word.push(c);
                State::Single
            }
            State::Double => step_double(c, &mut chars, &mut word),
        };
    }
    match state {
        State::Single => Err(ArgvError::UnterminatedSingleQuote),
        State::Double => Err(ArgvError::UnterminatedDoubleQuote),
        State::Unquoted => {
            words.push(word);
            Ok(words)
        }
        State::Between => Ok(words),
    }
}

fn step_unquoted(
    c: char,
    state: State,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    word: &mut String,
    words: &mut Vec<String>,
) -> Result<State, ArgvError> {
    match c {
        ' ' | '\t' | '\n' => {
            if state == State::Unquoted {
                words.push(std::mem::take(word));
            }
            Ok(State::Between)
        }
        '\'' => Ok(State::Single),
        '"' => Ok(State::Double),
        '\\' => match chars.next() {
            None => Err(ArgvError::TrailingBackslash),
            Some('\n') => Ok(state),
            Some(next) => {
                word.push(next);
                Ok(State::Unquoted)
            }
        },
        _ => {
            word.push(c);
            Ok(State::Unquoted)
        }
    }
}

fn step_double(
    c: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    word: &mut String,
) -> State {
    match c {
        '"' => State::Unquoted,
        '\\' => {
            match chars.peek().copied() {
                Some('\n') => {
                    chars.next();
                }
                Some(next @ ('$' | '`' | '"' | '\\')) => {
                    chars.next();
                    word.push(next);
                }
                _ => word.push('\\'),
            }
            State::Double
        }
        _ => {
            word.push(c);
            State::Double
        }
    }
}

/// Quotes a word so that [`split`] returns it unchanged.
pub fn quote(word: &str) -> String {
    let safe = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c));
    if safe {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// The words as one command line that [`split`] reads back as the same words.
pub fn join(words: &[String]) -> String {
    words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(input: &str) -> Vec<String> {
        split(input).unwrap()
    }

    #[test]
    fn splits_on_whitespace_runs() {
        assert_eq!(
            s("  oxlint\t--format  json\n"),
            ["oxlint", "--format", "json"]
        );
    }

    #[test]
    fn empty_input_is_no_words() {
        assert!(s("").is_empty());
        assert!(s("   ").is_empty());
    }

    #[test]
    fn sh_dash_c_with_a_pipeline_is_three_arguments() {
        assert_eq!(s(r#"sh -c "a | b""#), ["sh", "-c", "a | b"]);
    }

    #[test]
    fn operators_variables_and_globs_stay_literal() {
        assert_eq!(
            s("echo $HOME *.ts a|b c;d && e > f ~ #x"),
            [
                "echo", "$HOME", "*.ts", "a|b", "c;d", "&&", "e", ">", "f", "~", "#x"
            ]
        );
    }

    #[test]
    fn single_quotes_keep_everything_literal() {
        assert_eq!(s(r#"'a "b" \n $c'"#), [r#"a "b" \n $c"#]);
    }

    #[test]
    fn double_quotes_escape_only_special_characters() {
        assert_eq!(s(r#""a \" \\ \$ \` \n""#), [r#"a " \ $ ` \n"#]);
    }

    #[test]
    fn double_quoted_backslash_newline_is_a_continuation() {
        assert_eq!(s("\"a\\\nb\""), ["ab"]);
    }

    #[test]
    fn unquoted_backslash_escapes_the_next_character() {
        assert_eq!(s(r"a\ b \'c \\"), ["a b", "'c", r"\"]);
    }

    #[test]
    fn unquoted_backslash_newline_joins_lines() {
        assert_eq!(s("oxlint \\\n--fix"), ["oxlint", "--fix"]);
        assert_eq!(s("ox\\\nlint"), ["oxlint"]);
    }

    #[test]
    fn adjacent_quoted_parts_form_one_word() {
        assert_eq!(s(r#"a'b c'"d e"f"#), ["ab cd ef"]);
    }

    #[test]
    fn empty_quotes_are_an_empty_argument() {
        assert_eq!(s(r#"a '' """#), ["a", "", ""]);
    }

    #[test]
    fn unterminated_quotes_are_errors() {
        assert_eq!(split("a 'b"), Err(ArgvError::UnterminatedSingleQuote));
        assert_eq!(split(r#"a "b"#), Err(ArgvError::UnterminatedDoubleQuote));
        assert_eq!(split(r#"a "b\""#), Err(ArgvError::UnterminatedDoubleQuote));
    }

    #[test]
    fn trailing_backslash_is_an_error() {
        assert_eq!(split(r"a \"), Err(ArgvError::TrailingBackslash));
    }

    #[test]
    fn parses_placeholders_as_their_own_arguments() {
        let cmd = Command::parse("oxlint --format json {files}").unwrap();
        assert!(cmd.has_placeholder());
        assert_eq!(cmd.placeholders(), [Placeholder::Files]);
        assert_eq!(cmd.args().len(), 4);
    }

    #[test]
    fn a_command_without_placeholders_reports_none() {
        let cmd = Command::parse("cargo clippy --workspace").unwrap();
        assert!(!cmd.has_placeholder());
        assert!(cmd.placeholders().is_empty());
        assert_eq!(
            cmd.expand(&["x".into()], &["y".into()]),
            ["cargo", "clippy", "--workspace"]
        );
    }

    #[test]
    fn expands_each_placeholder_to_one_argument_per_item() {
        let cmd = Command::parse("tool {affected} -- {files} {files}").unwrap();
        assert_eq!(
            cmd.placeholders(),
            [Placeholder::Affected, Placeholder::Files]
        );
        let files = ["a b.ts".to_string(), "c.ts".to_string()];
        let affected = ["web".to_string()];
        assert_eq!(
            cmd.expand(&files, &affected),
            ["tool", "web", "--", "a b.ts", "c.ts", "a b.ts", "c.ts"]
        );
    }

    #[test]
    fn expanding_an_empty_list_yields_no_arguments() {
        let cmd = Command::parse("oxfmt --check {files}").unwrap();
        assert_eq!(cmd.expand(&[], &[]), ["oxfmt", "--check"]);
    }

    #[test]
    fn a_placeholder_embedded_in_a_word_is_an_error() {
        assert_eq!(
            Command::parse("tool --files={files}"),
            Err(ArgvError::EmbeddedPlaceholder {
                word: "--files={files}".into(),
                placeholder: Placeholder::Files,
            })
        );
        assert!(matches!(
            Command::parse("tool x{affected}"),
            Err(ArgvError::EmbeddedPlaceholder {
                placeholder: Placeholder::Affected,
                ..
            })
        ));
    }

    #[test]
    fn other_braces_are_literal() {
        let cmd = Command::parse("find . -exec rm {} ; {file}").unwrap();
        assert!(!cmd.has_placeholder());
    }

    #[test]
    fn a_quoted_or_escaped_placeholder_is_still_a_placeholder() {
        for run in ["x '{files}'", "x \"{files}\"", r"x \{files\}"] {
            let cmd = Command::parse(run).unwrap();
            assert_eq!(cmd.placeholders(), [Placeholder::Files], "{run}");
        }
    }

    #[test]
    fn a_hash_is_not_a_comment() {
        assert_eq!(s("a #b"), ["a", "#b"]);
    }

    #[test]
    fn an_empty_command_is_an_error() {
        assert_eq!(Command::parse("  "), Err(ArgvError::Empty));
    }

    #[test]
    fn split_errors_surface_from_parse() {
        assert_eq!(
            Command::parse("sh -c 'a"),
            Err(ArgvError::UnterminatedSingleQuote)
        );
    }

    #[test]
    fn quote_round_trips_through_split() {
        for word in [
            "plain/path.ts",
            "a b",
            "it's",
            "",
            "$x",
            "\"q\"",
            "a\\b",
            "{files}",
        ] {
            assert_eq!(s(&quote(word)), [word], "{word}");
        }
        assert_eq!(quote("src/App.tsx"), "src/App.tsx");
        assert_eq!(quote("a b"), "'a b'");
    }
}
