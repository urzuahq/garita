//! Risk classes and how a change is matched to one (RFC-3 §3).

use crate::config::EvidenceAccept;
use crate::glob;
use crate::report::{MatchedBy, Risk, RiskSource};

/// Paths that match the highest class whatever the policy says, since a change to them can
/// change the gate itself.
pub const BUILTIN_PROTECTED: [&str; 2] = [".garita/**", ".github/workflows/**"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskClass {
    pub name: String,
    /// The highest system a change of the class must pass.
    pub requires: u8,
    pub approvals: u64,
    pub author_counts: bool,
    pub evidence: Option<EvidenceAccept>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchRule {
    pub paths: Vec<String>,
    /// An index into [`RiskPolicy::classes`].
    pub class: usize,
}

/// The `risk` section of a config; parsing guarantees every index names a class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskPolicy {
    /// Lowest first; never empty.
    pub classes: Vec<RiskClass>,
    /// The class of an unmatched path; `None` gives it the highest class.
    pub default: Option<usize>,
    /// Added to [`BUILTIN_PROTECTED`].
    pub protected: Vec<String>,
    pub enforce: bool,
    pub rules: Vec<MatchRule>,
}

/// A path a change touches; a rename or copy also names the path it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    pub path: String,
    pub from: Option<String>,
}

impl Touched {
    pub fn path(path: &str) -> Self {
        Self {
            path: path.to_string(),
            from: None,
        }
    }

    pub fn renamed(from: &str, to: &str) -> Self {
        Self {
            path: to.to_string(),
            from: Some(from.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matched {
    /// An index into [`RiskPolicy::classes`].
    pub class: usize,
    pub source: RiskSource,
    pub matched_by: Vec<MatchedBy>,
}

impl RiskPolicy {
    fn highest(&self) -> usize {
        self.classes.len().saturating_sub(1)
    }

    pub fn default_class(&self) -> usize {
        self.default.unwrap_or_else(|| self.highest())
    }

    pub fn class(&self, index: usize) -> Option<&RiskClass> {
        self.classes.get(index)
    }

    /// The class `path` matches and the pattern that set it; `None` when nothing matches.
    fn match_path(&self, path: &str) -> Option<(usize, String)> {
        let protected = BUILTIN_PROTECTED
            .iter()
            .map(|p| (*p).to_string())
            .chain(self.protected.iter().cloned())
            .find(|p| glob::matches(p, path));
        if let Some(pattern) = protected {
            return Some((self.highest(), pattern));
        }
        let mut best: Option<(usize, String)> = None;
        for rule in &self.rules {
            if best.as_ref().is_some_and(|(class, _)| *class >= rule.class) {
                continue;
            }
            if let Some(pattern) = rule.paths.iter().find(|p| glob::matches(p, path)) {
                best = Some((rule.class, pattern.clone()));
            }
        }
        best
    }

    /// The change's class: the highest any touched path matches, a renamed path counting by
    /// its old name as well as its new one.
    pub fn match_change(&self, touched: &[Touched]) -> Matched {
        let default = self.default_class();
        let mut hits: Vec<(String, usize, String)> = Vec::new();
        let mut unmatched = false;
        let paths = touched
            .iter()
            .flat_map(|t| t.from.iter().chain(std::iter::once(&t.path)));
        for path in paths {
            if hits.iter().any(|(p, _, _)| p == path) {
                continue;
            }
            match self.match_path(path) {
                Some((class, pattern)) => hits.push((path.clone(), class, pattern)),
                None => unmatched = true,
            }
        }
        let top = hits.iter().map(|(_, class, _)| *class).max();
        match top {
            Some(top) if !(unmatched && default > top) => {
                let matched_by = hits
                    .into_iter()
                    .filter(|(_, class, _)| *class == top)
                    .map(|(path, class, pattern)| MatchedBy {
                        path,
                        pattern,
                        class: self.classes[class].name.clone(),
                    })
                    .collect();
                Matched {
                    class: top,
                    source: RiskSource::Matched,
                    matched_by,
                }
            }
            _ => Matched {
                class: default,
                source: RiskSource::Default,
                matched_by: Vec::new(),
            },
        }
    }

    /// The report's `risk` for a match, with no required checks yet.
    pub fn report(&self, matched: Matched) -> Risk {
        let class = &self.classes[matched.class];
        Risk {
            class: Some(class.name.clone()),
            source: matched.source,
            enforced: Some(self.enforce),
            requires: Some(class.requires),
            reached: None,
            met: None,
            approvals: Some(class.approvals),
            author_counts: Some(class.author_counts),
            needs_person: class.approvals >= 1,
            matched_by: matched.matched_by,
            required: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(name: &str) -> RiskClass {
        RiskClass {
            name: name.to_string(),
            requires: 0,
            approvals: 1,
            author_counts: false,
            evidence: None,
        }
    }

    fn rule(paths: &[&str], class: usize) -> MatchRule {
        MatchRule {
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
            class,
        }
    }

    fn policy(default: Option<usize>) -> RiskPolicy {
        RiskPolicy {
            classes: vec![class("routine"), class("core"), class("guarded")],
            default,
            protected: vec!["deploy/**".to_string()],
            enforce: true,
            rules: vec![
                rule(&["docs/**", "**/*.md"], 0),
                rule(&["crates/core/**"], 1),
                rule(&["crates/signing/**"], 2),
            ],
        }
    }

    fn by(path: &str, pattern: &str, class: &str) -> MatchedBy {
        MatchedBy {
            path: path.to_string(),
            pattern: pattern.to_string(),
            class: class.to_string(),
        }
    }

    fn paths(list: &[&str]) -> Vec<Touched> {
        list.iter().map(|p| Touched::path(p)).collect()
    }

    #[test]
    fn the_highest_matched_class_wins() {
        let m = policy(Some(0)).match_change(&paths(&["docs/a.md", "crates/core/lib.rs"]));
        assert_eq!(m.class, 1);
        assert_eq!(m.source, RiskSource::Matched);
        assert_eq!(
            m.matched_by,
            [by("crates/core/lib.rs", "crates/core/**", "core")]
        );
    }

    #[test]
    fn a_path_matching_several_rules_takes_the_highest() {
        let m = policy(Some(0)).match_change(&paths(&["crates/signing/README.md"]));
        assert_eq!(m.class, 2);
        assert_eq!(
            m.matched_by,
            [by(
                "crates/signing/README.md",
                "crates/signing/**",
                "guarded"
            )]
        );
    }

    #[test]
    fn every_path_that_set_the_class_is_listed_in_order() {
        let m = policy(Some(0)).match_change(&paths(&[
            "crates/core/a.rs",
            "docs/x.md",
            "crates/core/b.rs",
        ]));
        assert_eq!(
            m.matched_by,
            [
                by("crates/core/a.rs", "crates/core/**", "core"),
                by("crates/core/b.rs", "crates/core/**", "core"),
            ]
        );
    }

    #[test]
    fn an_unmatched_path_gets_the_default() {
        let m = policy(Some(1)).match_change(&paths(&["src/a.ts"]));
        assert_eq!(m.class, 1);
        assert_eq!(m.source, RiskSource::Default);
        assert!(m.matched_by.is_empty());
    }

    #[test]
    fn a_default_above_every_match_wins_as_the_default() {
        let m = policy(Some(1)).match_change(&paths(&["docs/a.md", "src/a.ts"]));
        assert_eq!((m.class, m.source), (1, RiskSource::Default));
        assert!(m.matched_by.is_empty());
    }

    #[test]
    fn a_match_equal_to_the_default_is_reported_as_matched() {
        let m = policy(Some(1)).match_change(&paths(&["crates/core/a.rs", "src/a.ts"]));
        assert_eq!((m.class, m.source), (1, RiskSource::Matched));
        assert_eq!(
            m.matched_by,
            [by("crates/core/a.rs", "crates/core/**", "core")]
        );
    }

    #[test]
    fn an_omitted_default_is_the_highest_class() {
        let m = policy(None).match_change(&paths(&["src/a.ts"]));
        assert_eq!((m.class, m.source), (2, RiskSource::Default));
    }

    #[test]
    fn an_empty_change_is_the_default() {
        let m = policy(Some(0)).match_change(&[]);
        assert_eq!((m.class, m.source), (0, RiskSource::Default));
        let m = policy(None).match_change(&[]);
        assert_eq!((m.class, m.source), (2, RiskSource::Default));
    }

    #[test]
    fn built_in_protected_paths_are_the_highest_class() {
        let mut p = policy(Some(0));
        p.rules.push(rule(&[".garita/**", ".github/**"], 0));
        for path in [".garita/config.yaml", ".github/workflows/ci.yml"] {
            let m = p.match_change(&paths(&[path]));
            assert_eq!((m.class, m.source), (2, RiskSource::Matched), "{path}");
        }
        let m = p.match_change(&paths(&[".garita/config.yaml"]));
        assert_eq!(
            m.matched_by,
            [by(".garita/config.yaml", ".garita/**", "guarded")]
        );
        let m = p.match_change(&paths(&[".github/CODEOWNERS"]));
        assert_eq!(m.class, 0);
    }

    #[test]
    fn protected_entries_add_to_the_built_in_set() {
        let m = policy(Some(0)).match_change(&paths(&["deploy/prod.sh"]));
        assert_eq!(m.class, 2);
        assert_eq!(m.matched_by, [by("deploy/prod.sh", "deploy/**", "guarded")]);
    }

    #[test]
    fn a_rename_matches_on_its_old_path() {
        let m =
            policy(Some(0)).match_change(&[Touched::renamed("crates/signing/k.rs", "docs/k.md")]);
        assert_eq!(m.class, 2);
        assert_eq!(
            m.matched_by,
            [by("crates/signing/k.rs", "crates/signing/**", "guarded")]
        );
    }

    #[test]
    fn a_rename_into_a_higher_class_matches_on_its_new_path() {
        let m = policy(Some(0)).match_change(&[Touched::renamed("docs/k.md", "crates/core/k.rs")]);
        assert_eq!(
            m.matched_by,
            [by("crates/core/k.rs", "crates/core/**", "core")]
        );
    }

    #[test]
    fn a_deleted_path_matches_like_any_other() {
        let m = policy(Some(0)).match_change(&paths(&["crates/signing/gone.rs"]));
        assert_eq!(m.class, 2);
    }

    #[test]
    fn a_path_touched_twice_is_listed_once() {
        let m = policy(Some(0)).match_change(&[
            Touched::path("crates/core/a.rs"),
            Touched::renamed("crates/core/a.rs", "crates/core/b.rs"),
        ]);
        assert_eq!(m.matched_by.len(), 2);
    }

    #[test]
    fn a_single_class_policy_is_always_that_class() {
        let p = RiskPolicy {
            classes: vec![class("only")],
            default: None,
            protected: Vec::new(),
            enforce: true,
            rules: Vec::new(),
        };
        let m = p.match_change(&paths(&["a", ".garita/x"]));
        assert_eq!((m.class, m.source), (0, RiskSource::Matched));
        assert_eq!(m.matched_by, [by(".garita/x", ".garita/**", "only")]);
    }
}
