use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DurationError {
    #[error("expected a duration such as 500ms, 3s, 10m or 1h")]
    Format,
    #[error("a duration must be greater than zero")]
    Zero,
}

/// Parses `500ms`, `3s`, `10m` or `1h`: a whole number above zero and one unit, nothing else.
pub fn parse_duration(text: &str) -> Result<Duration, DurationError> {
    match millis(text) {
        Some(0) => Err(DurationError::Zero),
        Some(n) => Ok(Duration::from_millis(n)),
        None => Err(DurationError::Format),
    }
}

fn millis(text: &str) -> Option<u64> {
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (digits, unit) = text.split_at(split);
    if digits.is_empty() {
        return None;
    }
    let n: u64 = digits.parse().ok()?;
    match unit {
        "ms" => Some(n),
        "s" => n.checked_mul(1_000),
        "m" => n.checked_mul(60_000),
        "h" => n.checked_mul(3_600_000),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_unit() {
        assert_eq!(parse_duration("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_duration("3s"), Ok(Duration::from_secs(3)));
        assert_eq!(parse_duration("10m"), Ok(Duration::from_secs(600)));
        assert_eq!(parse_duration("1h"), Ok(Duration::from_secs(3600)));
    }

    #[test]
    fn rejects_a_zero_duration_in_any_unit() {
        for zero in ["0s", "0ms", "00m", "0h"] {
            assert_eq!(parse_duration(zero), Err(DurationError::Zero), "{zero}");
        }
        assert_eq!(
            DurationError::Zero.to_string(),
            "a duration must be greater than zero"
        );
    }

    #[test]
    fn rejects_anything_else() {
        for bad in [
            "",
            "3",
            "s",
            "1.5s",
            "-1s",
            "+1s",
            "3 s",
            " 3s",
            "3s ",
            "3sec",
            "3S",
            "1d",
            "1h30m",
            "3ms5",
            "99999999999999999999s",
            "18446744073709551615h",
        ] {
            assert_eq!(parse_duration(bad), Err(DurationError::Format), "{bad}");
        }
    }
}
