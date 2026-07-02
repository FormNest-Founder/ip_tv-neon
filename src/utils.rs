use chrono::{DateTime, NaiveDateTime, Utc};
use std::fs::OpenOptions;
use std::io::Write;

pub fn main_log(msg: &str) {
    let log_path = crate::consts::get_cache_dir().join("neon_iptv.log");
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| writeln!(f, "[{}] {}", Utc::now().format("%H:%M:%S"), msg));
}

pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Strip terminal control characters from untrusted text before it reaches any
/// TTY/ratatui sink (CG8). Replacing every C0/C1 control char — including ESC
/// (0x1B) and BEL (0x07) — with a space neutralizes ANSI CSI / OSC sequences:
/// once the introducer ESC is gone the trailing bytes are inert printable text.
pub fn sanitize_terminal(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Parse an XMLTV timestamp into a Unix epoch, or 0 if unrecognized.
///
/// Accepts, in order: `YYYYMMDDHHMMSS +ZZZZ` (with explicit offset),
/// `YYYYMMDDHHMMSS` and `YYYYMMDDHHMM` (minutes-only). A timestamp without an
/// explicit timezone offset is assumed to be naive UTC — XMLTV feeds without a
/// `+ZZZZ` suffix are treated as UTC wall-clock here.
pub fn parse_xml_time(s: &str) -> i64 {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y%m%d%H%M%S %z") {
        return dt.timestamp();
    }
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y%m%d%H%M %z") {
        return dt.timestamp();
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M%S") {
        return dt.and_utc().timestamp();
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M") {
        return dt.and_utc().timestamp();
    }
    0
}

#[cfg(test)]
mod tests {
    use super::parse_xml_time;
    use chrono::{DateTime, NaiveDateTime};

    #[test]
    fn parses_full_with_offset() {
        let want = DateTime::parse_from_str("20240115123000 +0300", "%Y%m%d%H%M%S %z")
            .unwrap()
            .timestamp();
        assert_eq!(parse_xml_time("20240115123000 +0300"), want);
    }

    #[test]
    fn parses_naive_seconds_as_utc() {
        let want = NaiveDateTime::parse_from_str("20240115123000", "%Y%m%d%H%M%S")
            .unwrap()
            .and_utc()
            .timestamp();
        assert_eq!(parse_xml_time("20240115123000"), want);
    }

    #[test]
    fn parses_minutes_only() {
        let want = NaiveDateTime::parse_from_str("202401151230", "%Y%m%d%H%M")
            .unwrap()
            .and_utc()
            .timestamp();
        assert_eq!(parse_xml_time("202401151230"), want);
        assert!(parse_xml_time("202401151230") > 0);
    }

    #[test]
    fn parses_minutes_only_with_offset() {
        assert!(parse_xml_time("202401151230 +0000") > 0);
    }

    #[test]
    fn garbage_returns_zero() {
        assert_eq!(parse_xml_time("not-a-time"), 0);
        assert_eq!(parse_xml_time(""), 0);
        assert_eq!(parse_xml_time("2024"), 0);
    }

    #[test]
    fn trims_whitespace() {
        assert!(parse_xml_time("  20240115123000  ") > 0);
    }
}
