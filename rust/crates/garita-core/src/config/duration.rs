use std::time::Duration;

/// Parses `500ms`, `3s`, `10m` or `1h`: a whole number and one unit, nothing else.
pub fn parse_duration(text: &str) -> Option<Duration> {
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (digits, unit) = text.split_at(split);
    if digits.is_empty() {
        return None;
    }
    let n: u64 = digits.parse().ok()?;
    let millis = match unit {
        "ms" => Some(n),
        "s" => n.checked_mul(1_000),
        "m" => n.checked_mul(60_000),
        "h" => n.checked_mul(3_600_000),
        _ => None,
    }?;
    Some(Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_unit() {
        assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration("3s"), Some(Duration::from_secs(3)));
        assert_eq!(parse_duration("10m"), Some(Duration::from_secs(600)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("0s"), Some(Duration::ZERO));
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
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }
}
