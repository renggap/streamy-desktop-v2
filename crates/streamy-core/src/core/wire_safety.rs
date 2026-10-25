//! Wire safety and input sanitization for network headers and titles.

pub const MAX_HEADER_VALUE: usize = 512;
pub const MAX_TITLE: usize = 1024;

/// Strip ASCII control characters (CR, LF, NUL, etc.) and truncate to `max`.
pub fn header_value(value: &str, max: usize) -> String {
    truncate(&strip_control_chars(value), max)
}

/// Strip ASCII control chars (0x00–0x1F and 0x7F) but keep UTF-8 content bytes intact.
pub fn strip_control_chars(value: &str) -> String {
    value
        .chars()
        .filter(|&c| {
            let code = c as u32;
            code >= 0x20 && code != 0x7F
        })
        .collect()
}

/// Truncate to `max` UTF-16 code units without splitting a surrogate pair (which would emit a lone surrogate).
pub fn truncate(value: &str, max: usize) -> String {
    let utf16: Vec<u16> = value.encode_utf16().collect();
    if utf16.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    if end > 0 && is_high_surrogate(utf16[end - 1]) && is_low_surrogate(utf16[end]) {
        end -= 1;
    }
    String::from_utf16_lossy(&utf16[..end])
}

#[inline]
fn is_high_surrogate(code: u16) -> bool {
    (0xD800..=0xDBFF).contains(&code)
}

#[inline]
fn is_low_surrogate(code: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&code)
}

/// Parse the numeric HTTP status code out of a status line like `HTTP/1.0 200 OK`.
/// Returns the code as i32, or -1 when the line isn't a valid status line.
/// Never matches on a substring — "HTTP/1.0 2000 Broken" or "garbage with 200 inside" returns -1.
pub fn parse_http_status(status_line: &str) -> i32 {
    let parts: Vec<&str> = status_line.split_whitespace().collect();
    if parts.len() < 2 {
        return -1;
    }
    if !parts[0].starts_with("HTTP/") {
        return -1;
    }
    if let Ok(code) = parts[1].parse::<i32>() {
        if (100..=999).contains(&code) {
            return code;
        }
    }
    -1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_status_extracts_numeric_code() {
        assert_eq!(parse_http_status("HTTP/1.0 200 OK"), 200);
        assert_eq!(parse_http_status("HTTP/1.1 401 Unauthorized"), 401);
        assert_eq!(parse_http_status("HTTP/1.0 404 Not found"), 404);
    }

    #[test]
    fn parse_http_status_rejects_substring_and_nonstatus_lines() {
        assert_eq!(parse_http_status("HTTP/1.0 2000 Broken"), -1);
        assert_eq!(parse_http_status("garbage with 200 inside"), -1);
        assert_eq!(parse_http_status(""), -1);
        assert_eq!(parse_http_status("OK"), -1);
        assert_eq!(parse_http_status("HTTP/1.0 OK"), -1);
    }

    #[test]
    fn parse_http_status_tolerates_extra_whitespace() {
        assert_eq!(parse_http_status("HTTP/1.0  200  OK"), 200);
        assert_eq!(parse_http_status("  HTTP/1.0 200 OK  "), 200);
        assert_eq!(parse_http_status("HTTP/1.0 2000 Broken"), -1);
    }

    #[test]
    fn header_value_strips_crlf_and_controls() {
        let clean = header_value("Bad\r\nInjected: header\u{0000}Name", MAX_HEADER_VALUE);
        assert_eq!(clean, "BadInjected: headerName");
        assert!(!clean.contains('\r'));
        assert!(!clean.contains('\n'));
        assert!(!clean.contains('\0'));
    }

    #[test]
    fn header_value_truncates_to_cap() {
        let long = "x".repeat(10_000);
        assert_eq!(header_value(&long, MAX_HEADER_VALUE).len(), MAX_HEADER_VALUE);
    }

    #[test]
    fn truncate_splits_at_cap_exactly() {
        assert_eq!(truncate("abcdef", 3), "abc");
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn truncate_never_splits_surrogate_pair() {
        // Emoji = 2 UTF-16 code units. Cap landing between halves backs off one code unit.
        let s = format!("abc\u{1F600}{}", "y".repeat(10));
        let out = truncate(&s, 4);
        assert_eq!(out, "abc");

        let out2 = truncate(&s, 5);
        assert_eq!(out2, "abc\u{1F600}");
    }

    #[test]
    fn strip_control_chars_keeps_utf8_content() {
        let s = "DJ \u{00e9}\u{00e8} Titel\r\n";
        let out = strip_control_chars(s);
        assert_eq!(out, "DJ \u{00e9}\u{00e8} Titel");
        assert!(!out.contains('\r'));
        assert!(!out.contains('\n'));
    }
}
