//! Time stamps, run ids and the machine's name.

use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// `2026-11-21T03:15:02Z`: RFC 3339 in UTC, sub-second part dropped (rounded toward the past).
pub fn rfc3339(time: SystemTime) -> String {
    let secs: i64 = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => {
            let before = e.duration();
            let whole = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            if before.subsec_nanos() > 0 {
                whole.saturating_neg().saturating_sub(1)
            } else {
                whole.saturating_neg()
            }
        }
    };
    let days = secs.div_euclid(86_400);
    let of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A fresh ULID for a run id.
pub fn run_id() -> String {
    ulid::Ulid::generate().to_string()
}

/// The host name, or `unknown` when none can be found.
pub fn machine() -> String {
    hostname().unwrap_or_else(|| "unknown".to_string())
}

fn hostname() -> Option<String> {
    let clean = |s: String| {
        let s = s.trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .and_then(clean)
        .or_else(|| std::env::var("HOSTNAME").ok().and_then(clean))
        .or_else(|| {
            let out = Command::new("uname")
                .arg("-n")
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
                .and_then(clean)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> String {
        rfc3339(UNIX_EPOCH + Duration::from_secs(secs))
    }

    #[test]
    fn formats_known_instants() {
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(1_795_230_902), "2026-11-21T03:15:02Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(at(1_709_208_000), "2024-02-29T12:00:00Z");
        assert_eq!(at(946_684_799), "1999-12-31T23:59:59Z");
        assert_eq!(at(946_684_800), "2000-01-01T00:00:00Z");
        assert_eq!(at(4_107_542_400), "2100-03-01T00:00:00Z");
        assert_eq!(at(253_402_300_799), "9999-12-31T23:59:59Z");
    }

    #[test]
    fn drops_sub_second_digits() {
        let t = UNIX_EPOCH + Duration::from_millis(1_500);
        assert_eq!(rfc3339(t), "1970-01-01T00:00:01Z");
    }

    #[test]
    fn handles_times_before_the_epoch() {
        let t = UNIX_EPOCH - Duration::from_millis(500);
        assert_eq!(rfc3339(t), "1969-12-31T23:59:59Z");
        assert_eq!(
            rfc3339(UNIX_EPOCH - Duration::from_secs(86_400)),
            "1969-12-31T00:00:00Z"
        );
    }

    #[test]
    fn run_ids_are_ulids_that_name_a_receipt() {
        let id = run_id();
        assert_eq!(id.len(), 26);
        assert_ne!(id, run_id());
        let name = garita_core::receipt::file_name(
            "2026-11-21T03:15:02Z",
            garita_core::report::Trigger::Ci,
            &id,
        );
        assert!(name.is_ok(), "{name:?}");
    }

    #[test]
    fn machine_is_never_empty() {
        assert!(!machine().is_empty());
    }
}
