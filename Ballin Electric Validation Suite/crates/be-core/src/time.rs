//! UTC timestamps (RFC 3339) and monotonic duration helpers, without
//! external date libraries.

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A UTC wall-clock timestamp in milliseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct UtcMillis(pub i64);

impl UtcMillis {
    pub fn now() -> Self {
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        UtcMillis(d.as_millis() as i64)
    }

    /// RFC 3339 representation, e.g. `2026-11-25T14:20:01.123Z`.
    pub fn to_rfc3339(self) -> String {
        let (y, mo, d, h, mi, s, ms) = civil(self.0);
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z")
    }

    /// `HH:MM:SS` (UTC), used for compact event log lines.
    pub fn hms(self) -> String {
        let (_, _, _, h, mi, s, _) = civil(self.0);
        format!("{h:02}:{mi:02}:{s:02}")
    }

    pub fn date(self) -> String {
        let (y, mo, d, ..) = civil(self.0);
        format!("{y:04}-{mo:02}-{d:02}")
    }

    pub fn parse_rfc3339(s: &str) -> Option<UtcMillis> {
        // Accepts YYYY-MM-DDTHH:MM:SS[.mmm]Z
        let s = s.trim();
        let b = s.as_bytes();
        if b.len() < 20 || !s.ends_with('Z') {
            return None;
        }
        let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
        let y = num(0..4)?;
        let mo = num(5..7)?;
        let d = num(8..10)?;
        let h = num(11..13)?;
        let mi = num(14..16)?;
        let sec = num(17..19)?;
        let ms = if b.get(19) == Some(&b'.') {
            num(20..23).unwrap_or(0)
        } else {
            0
        };
        let days = days_from_civil(y, mo as u32, d as u32);
        Some(UtcMillis(
            ((days * 86400 + h * 3600 + mi * 60 + sec) * 1000) + ms,
        ))
    }
}

fn civil(ms: i64) -> (i64, u32, u32, u32, u32, u32, u32) {
    let secs = ms.div_euclid(1000);
    let msr = ms.rem_euclid(1000) as u32;
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    // Howard Hinnant's civil_from_days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        m,
        d,
        (sod / 3600) as u32,
        ((sod % 3600) / 60) as u32,
        (sod % 60) as u32,
        msr,
    )
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Format a duration as `HH:MM:SS`.
pub fn format_duration_ms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates() {
        assert_eq!(UtcMillis(0).to_rfc3339(), "1970-01-01T00:00:00.000Z");
        // 2000-02-29T12:34:56.789Z (leap day)
        let t = UtcMillis(951_827_696_789);
        assert_eq!(t.to_rfc3339(), "2000-02-29T12:34:56.789Z");
        assert_eq!(UtcMillis::parse_rfc3339(&t.to_rfc3339()), Some(t));
    }

    #[test]
    fn round_trip_many() {
        for ms in [
            1i64,
            86_399_999,
            1_700_000_000_000,
            4_102_444_800_000,
            1_798_243_200_123,
        ] {
            let t = UtcMillis(ms);
            assert_eq!(UtcMillis::parse_rfc3339(&t.to_rfc3339()), Some(t));
        }
    }

    #[test]
    fn duration_format() {
        assert_eq!(format_duration_ms(0), "00:00:00");
        assert_eq!(format_duration_ms(4_800_000), "01:20:00");
        assert_eq!(format_duration_ms(1_055_999), "00:17:35");
    }
}
