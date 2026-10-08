//! Human-readable formatting ported from the web prototype, implemented
//! without a datetime dependency (UTC civil-date math).

/// Short month names, `MONTHS_SHORT[m - 1]`.
const MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Days-since-Unix-epoch to `(year, month, day)` in UTC
/// (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `1234567` → `"1.2 MB"` (trailing zero decimals trimmed, like the web UI).
pub fn format_bytes(bytes: u64, decimals: usize) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let i = ((bytes as f64).log(1024.0).floor() as usize).min(UNITS.len() - 1);
    let value = bytes as f64 / 1024f64.powi(i as i32);
    let mut rendered = format!("{value:.decimals$}");
    if rendered.contains('.') {
        rendered = rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
    }
    format!("{rendered} {}", UNITS[i])
}

/// Transfer speed, e.g. `"3.2 MB/s"`.
pub fn format_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec, 1))
}

/// `Oct 4, 2026 14:30` from epoch milliseconds (UTC).
pub fn format_date(timestamp_ms: i64) -> String {
    let secs = timestamp_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm) = (secs_of_day / 3_600, (secs_of_day % 3_600) / 60);
    format!(
        "{} {}, {} {:02}:{:02}",
        MONTHS_SHORT[(m - 1) as usize],
        d,
        y,
        hh,
        mm
    )
}

/// `"just now"`, `"5m ago"`, `"3h ago"`, `"2d ago"`.
pub fn format_relative_time(now_ms: i64, timestamp_ms: i64) -> String {
    let diff = now_ms.saturating_sub(timestamp_ms).max(0);
    let minutes = diff / 60_000;
    if minutes < 1 {
        "just now".to_string()
    } else if minutes < 60 {
        format!("{minutes}m ago")
    } else if minutes < 24 * 60 {
        format!("{}h ago", minutes / 60)
    } else {
        format!("{}d ago", minutes / (24 * 60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_formatting_matches_web_prototype() {
        assert_eq!(format_bytes(0, 1), "0 B");
        assert_eq!(format_bytes(900, 1), "900 B");
        assert_eq!(format_bytes(1024, 1), "1 KB");
        assert_eq!(format_bytes(4_200_000, 1), "4 MB"); // 4.0 → 4
        assert_eq!(format_bytes(1_500_000_000, 2), "1.4 GB");
    }

    #[test]
    fn speed_formatting() {
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_speed(3_300_000), "3.1 MB/s");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_730), (2026, 10, 4));
    }

    #[test]
    fn date_formatting() {
        assert_eq!(format_date(0), "Jan 1, 1970 00:00");
        // 2026-10-04T14:30Z = 20730 days + 52_200 s.
        let ts = 20_730 * 86_400_000 + 52_200_000;
        assert_eq!(format_date(ts), "Oct 4, 2026 14:30");
    }

    #[test]
    fn relative_time_buckets() {
        let now = 1_000_000_000;
        assert_eq!(format_relative_time(now, now - 30_000), "just now");
        assert_eq!(format_relative_time(now, now - 5 * 60_000), "5m ago");
        assert_eq!(format_relative_time(now, now - 3 * 3_600_000), "3h ago");
        assert_eq!(format_relative_time(now, now - 2 * 86_400_000), "2d ago");
    }
}
