use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VersionError {
    #[error("`{0}` is not a version of the form MAJOR.MINOR.PATCH")]
    Version(String),
    #[error(
        "`{0}` is not a version requirement; use comparisons such as `>=0.1.0` or `>=0.1.0, <1.0.0`"
    )]
    Requirement(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn parse(text: &str) -> Result<Self, VersionError> {
        let err = || VersionError::Version(text.to_string());
        let parts: Vec<&str> = text.split('.').collect();
        let [major, minor, patch] = parts.as_slice() else {
            return Err(err());
        };
        let num = |p: &str| {
            let digits = !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
            digits
                .then(|| p.parse::<u64>().ok())
                .flatten()
                .ok_or_else(err)
        };
        Ok(Self {
            major: num(major)?,
            minor: num(minor)?,
            patch: num(patch)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}

/// A conjunction of comparisons, such as `>=0.1.0, <1.0.0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    pub comparators: Vec<(Op, Version)>,
    pub text: String,
}

impl VersionReq {
    pub fn parse(text: &str) -> Result<Self, VersionError> {
        let err = || VersionError::Requirement(text.to_string());
        let comparators = text
            .split(',')
            .map(|part| comparator(part.trim()).ok_or_else(err))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            comparators,
            text: text.to_string(),
        })
    }

    pub fn matches(&self, version: Version) -> bool {
        self.comparators.iter().all(|(op, bound)| match op {
            Op::Eq => version == *bound,
            Op::Gt => version > *bound,
            Op::Ge => version >= *bound,
            Op::Lt => version < *bound,
            Op::Le => version <= *bound,
        })
    }
}

fn comparator(part: &str) -> Option<(Op, Version)> {
    let (op, rest) = [
        (">=", Op::Ge),
        ("<=", Op::Le),
        (">", Op::Gt),
        ("<", Op::Lt),
        ("=", Op::Eq),
    ]
    .into_iter()
    .find_map(|(sym, op)| part.strip_prefix(sym).map(|rest| (op, rest)))
    .unwrap_or((Op::Eq, part));
    Version::parse(rest.trim_start()).ok().map(|v| (op, v))
}

/// Whether `garita_version` may run a config that `requires` this.
pub fn satisfies(requires: &VersionReq, garita_version: &str) -> Result<bool, VersionError> {
    Version::parse(garita_version).map(|v| requires.matches(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str) -> VersionReq {
        VersionReq::parse(text).unwrap()
    }

    #[test]
    fn parses_versions() {
        assert_eq!(
            Version::parse("1.20.3"),
            Ok(Version {
                major: 1,
                minor: 20,
                patch: 3
            })
        );
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.x",
            "1.2.3-dev",
            "1..3",
            "+1.2.3",
        ] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn each_operator_compares() {
        let v = |t: &str| Version::parse(t).unwrap();
        assert!(req(">=0.1.0").matches(v("0.1.0")));
        assert!(req(">=0.1.0").matches(v("0.2.0")));
        assert!(!req(">=0.1.0").matches(v("0.0.9")));
        assert!(req(">0.1.0").matches(v("0.1.1")));
        assert!(!req(">0.1.0").matches(v("0.1.0")));
        assert!(req("<1.0.0").matches(v("0.9.9")));
        assert!(!req("<1.0.0").matches(v("1.0.0")));
        assert!(req("<=1.0.0").matches(v("1.0.0")));
        assert!(req("=1.0.0").matches(v("1.0.0")));
        assert!(req("1.0.0").matches(v("1.0.0")));
        assert!(!req("1.0.0").matches(v("1.0.1")));
        assert!(req(">= 0.10.0").matches(v("0.10.0")));
    }

    #[test]
    fn comparisons_are_numeric_not_textual() {
        assert!(req(">=0.9.0").matches(Version::parse("0.10.0").unwrap()));
    }

    #[test]
    fn a_comma_is_a_conjunction() {
        let r = req(">=0.1.0, <1.0.0");
        assert!(satisfies(&r, "0.5.0").unwrap());
        assert!(!satisfies(&r, "1.0.0").unwrap());
        assert!(!satisfies(&r, "0.0.1").unwrap());
    }

    #[test]
    fn rejects_unsupported_requirements() {
        for bad in ["", "^0.1.0", "~0.1", ">=0.1", ">=0.1.0,", "*", ">>1.0.0"] {
            assert_eq!(
                VersionReq::parse(bad),
                Err(VersionError::Requirement(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn satisfies_rejects_a_bad_garita_version() {
        assert_eq!(
            satisfies(&req(">=0.1.0"), "dev"),
            Err(VersionError::Version("dev".into()))
        );
    }
}
