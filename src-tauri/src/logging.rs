//! Rolling log appender and credential redaction.
//! Logs to `%APPDATA%\com.streamy.desktop\logs\` with passwords/auth tokens stripped.

use std::path::PathBuf;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// Redact credentials, auth headers, and passwords from log strings.
pub fn redact_credentials(msg: &str) -> String {
    // Redact Basic auth
    let mut out = msg.to_string();
    if let Some(idx) = out.find("Basic ") {
        let rest = &out[idx + 6..];
        let end = rest.find(|c: char| c.is_whitespace() || c == '"' || c == '\'').unwrap_or(rest.len());
        if !rest.starts_with("[REDACTED]") {
            out.replace_range(idx + 6..idx + 6 + end, "[REDACTED]");
        }
    }

    // Redact password="..." or password: "..."
    let patterns = ["password=", "password:", "password\": \"", "passwd=", "pwd="];
    for pat in patterns {
        let mut search_from = 0;
        while let Some(rel_idx) = out[search_from..].to_lowercase().find(pat) {
            let idx = search_from + rel_idx;
            let start = idx + pat.len();
            let rest = &out[start..];
            let end = rest.find(|c: char| c == ',' || c == '}' || c == '&' || c == '"' || c == '\n' || c == '\r').unwrap_or(rest.len());
            if end > 0 {
                out.replace_range(start..start + end, "[REDACTED]");
                search_from = start + "[REDACTED]".len();
            } else {
                search_from = start;
            }
        }
    }
    out
}

pub fn init_logging(log_dir: PathBuf) -> Result<(), String> {
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "streamy.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,streamy_core=debug,streamy_desktop=debug"));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false);

    let stdout_layer = fmt::layer()
        .with_ansi(true);

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stdout_layer)
        .try_init();

    // Leak the guard so logging runs for process lifetime
    Box::leak(Box::new(_guard));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_credentials() {
        let line = "Connecting with Authorization: Basic c291cmNlOnNlY3JldA== to server";
        let redacted = redact_credentials(line);
        assert!(!redacted.contains("c291cmNlOnNlY3JldA=="));
        assert!(redacted.contains("Basic [REDACTED]"));

        let line2 = "Profile config: password=SuperSecretPassword123&user=admin";
        let redacted2 = redact_credentials(line2);
        assert!(!redacted2.contains("SuperSecretPassword123"));
        assert!(redacted2.contains("password=[REDACTED]"));
    }
}
