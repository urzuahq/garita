//! Globs over repository-relative paths: `*`, `**`, `?`, `[a-z]`, `[!a]` and `{a,b}`.

const MAX_ALTERNATIVES: usize = 1024;

/// Whether `pattern` matches the whole of `path`; both use `/` separators.
pub fn matches(pattern: &str, path: &str) -> bool {
    let path: Vec<&str> = segments(path);
    expand_braces(pattern)
        .iter()
        .any(|p| match_segments(&segments(p), &path))
}

/// Whether any of `patterns` matches `path`.
pub fn any_matches(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|p| matches(p, path))
}

fn segments(text: &str) -> Vec<&str> {
    let text = text.strip_prefix("./").unwrap_or(text);
    text.split('/').filter(|s| !s.is_empty()).collect()
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            let rest = skip_globstars(rest);
            (0..=path.len()).any(|i| path.get(i..).is_some_and(|p| match_segments(rest, p)))
        }
        Some((segment, rest)) => path.split_first().is_some_and(|(first, tail)| {
            match_segment(segment, first) && match_segments(rest, tail)
        }),
    }
}

fn skip_globstars<'a, 'b>(mut pattern: &'a [&'b str]) -> &'a [&'b str] {
    while let Some((&"**", rest)) = pattern.split_first() {
        pattern = rest;
    }
    pattern
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(char),
    AnyOne,
    AnyRun,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

impl Token {
    fn accepts(&self, c: char) -> bool {
        match self {
            Token::Literal(l) => *l == c,
            Token::AnyOne => true,
            Token::AnyRun => false,
            Token::Class { negated, ranges } => {
                ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c)) != *negated
            }
        }
    }
}

fn tokens(segment: &str) -> Vec<Token> {
    let chars: Vec<char> = segment.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        i += 1;
        match c {
            '*' => out.push(Token::AnyRun),
            '?' => out.push(Token::AnyOne),
            '\\' => {
                if let Some(&next) = chars.get(i) {
                    out.push(Token::Literal(next));
                    i += 1;
                } else {
                    out.push(Token::Literal('\\'));
                }
            }
            '[' => match class(&chars, i) {
                Some((token, next)) => {
                    out.push(token);
                    i = next;
                }
                None => out.push(Token::Literal('[')),
            },
            other => out.push(Token::Literal(other)),
        }
    }
    out
}

/// A character class starting just after `[`; `None` when it is never closed.
fn class(chars: &[char], start: usize) -> Option<(Token, usize)> {
    let mut i = start;
    let negated = matches!(chars.get(i), Some('!' | '^'));
    if negated {
        i += 1;
    }
    let mut ranges = Vec::new();
    let mut first = true;
    loop {
        let c = *chars.get(i)?;
        if c == ']' && !first {
            return Some((Token::Class { negated, ranges }, i + 1));
        }
        first = false;
        let hi = match (chars.get(i + 1), chars.get(i + 2)) {
            (Some('-'), Some(&hi)) if hi != ']' => {
                i += 2;
                hi
            }
            _ => c,
        };
        ranges.push((c, hi));
        i += 1;
    }
}

/// Matches one path segment; `*` never crosses a `/`, since segments hold none.
fn match_segment(pattern: &str, text: &str) -> bool {
    let pattern = tokens(pattern);
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        let c = text.get(t).copied().unwrap_or_default();
        match pattern.get(p) {
            Some(Token::AnyRun) => {
                star = Some((p, t));
                p += 1;
            }
            Some(token) if token.accepts(c) => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    p = sp + 1;
                    t = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    pattern
        .get(p..)
        .is_some_and(|rest| rest.iter().all(|t| *t == Token::AnyRun))
}

/// `a{b,c{d,e}}` becomes `ab`, `acd`, `ace`; a brace group with no top-level comma is literal.
fn expand_braces(pattern: &str) -> Vec<String> {
    let mut out = Vec::new();
    expand_into(pattern, &mut out);
    out
}

fn expand_into(pattern: &str, out: &mut Vec<String>) {
    if out.len() >= MAX_ALTERNATIVES {
        return;
    }
    let Some((open, close, options)) = first_group(pattern) else {
        out.push(pattern.to_string());
        return;
    };
    let head = pattern.get(..open).unwrap_or_default();
    let tail = pattern.get(close + 1..).unwrap_or_default();
    for option in options {
        expand_into(&format!("{head}{option}{tail}"), out);
    }
}

/// The first `{...}` group that has a top-level comma: its byte span and its options.
fn first_group(pattern: &str) -> Option<(usize, usize, Vec<&str>)> {
    let bytes = pattern.as_bytes();
    let mut search = 0;
    while let Some(offset) = pattern.get(search..)?.find('{') {
        let open = search + offset;
        let mut depth = 0usize;
        let mut commas = Vec::new();
        let mut close = None;
        let mut i = open;
        while let Some(&b) = bytes.get(i) {
            match b {
                b'\\' => i += 1,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                b',' if depth == 1 => commas.push(i),
                _ => {}
            }
            i += 1;
        }
        match close {
            Some(close) if !commas.is_empty() => {
                let mut options = Vec::new();
                let mut start = open + 1;
                for comma in commas.into_iter().chain([close]) {
                    options.push(pattern.get(start..comma)?);
                    start = comma + 1;
                }
                return Some((open, close, options));
            }
            _ => search = open + 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_stays_within_a_segment() {
        assert!(matches("*.ts", "a.ts"));
        assert!(!matches("*.ts", "src/a.ts"));
        assert!(matches("src/*.ts", "src/a.ts"));
        assert!(matches("src/*", "src/a.ts"));
        assert!(!matches("src/*", "src"));
        assert!(matches("a*b*c", "aXbYc"));
        assert!(!matches("a*b*c", "aXbY"));
    }

    #[test]
    fn globstar_spans_any_number_of_segments_including_none() {
        assert!(matches("**/*.ts", "a.ts"));
        assert!(matches("**/*.ts", "x/y/z/a.ts"));
        assert!(matches("src/**/a.ts", "src/a.ts"));
        assert!(matches("src/**/a.ts", "src/x/y/a.ts"));
        assert!(matches("src/**", "src/x/y"));
        assert!(matches("**", "anything/at/all"));
        assert!(!matches("src/**/a.ts", "lib/a.ts"));
        assert!(matches("**/**/a", "a"));
    }

    #[test]
    fn question_mark_and_classes() {
        assert!(matches("a?.ts", "ab.ts"));
        assert!(!matches("a?.ts", "a.ts"));
        assert!(matches("[abc].ts", "b.ts"));
        assert!(!matches("[abc].ts", "d.ts"));
        assert!(matches("[a-z][0-9].ts", "q7.ts"));
        assert!(!matches("[a-z][0-9].ts", "Q7.ts"));
        assert!(matches("[!a].ts", "b.ts"));
        assert!(!matches("[!a].ts", "a.ts"));
        assert!(matches("[]].ts", "].ts"));
        assert!(matches("[a-].ts", "-.ts"));
        assert!(matches("[.ts", "[.ts"));
    }

    #[test]
    fn braces_are_alternatives_and_nest() {
        assert!(matches("**/*.{ts,tsx}", "src/a.tsx"));
        assert!(matches("**/*.{ts,tsx}", "src/a.ts"));
        assert!(!matches("**/*.{ts,tsx}", "src/a.js"));
        assert!(matches("{src,lib/{a,b}}/x", "lib/b/x"));
        assert!(matches("{a}.ts", "{a}.ts"));
        assert!(matches("{packages/*,apps/*}", "apps/web"));
    }

    #[test]
    fn patterns_match_the_whole_path() {
        assert!(!matches("a.ts", "src/a.ts"));
        assert!(!matches("src", "src/a.ts"));
        assert!(matches("./src/a.ts", "src/a.ts"));
        assert!(matches(r"\*.ts", "*.ts"));
        assert!(!matches(r"\*.ts", "a.ts"));
    }

    #[test]
    fn any_matches_needs_one() {
        let patterns = vec!["*.md".to_string(), "**/*.ts".to_string()];
        assert!(any_matches(&patterns, "a/b.ts"));
        assert!(!any_matches(&patterns, "a/b.rs"));
        assert!(!any_matches(&[], "a"));
    }
}
