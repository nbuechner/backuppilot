//! Desktop notifications via `notify-send` (GNOME / freedesktop).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::app_settings::load_app_settings;
use crate::ids::ICON_NAME as ICON;

const APP_NAME: &str = "BackupPilot";
const PROGRESS_MIN_INTERVAL: Duration = Duration::from_secs(2);
const FINISH_EXPIRE_MS: u32 = 10_000;

/// Show a libnotify notification if `notify-send` is available.
pub fn send_desktop_notification(summary: &str, body: &str) {
    let _ = spawn_notify_send(summary, body, None, FINISH_EXPIRE_MS, &[]);
}

/// Live backup progress in the GNOME notification center (replaces the same notification).
pub struct BackupProgressNotifier {
    summary: String,
    notification_id: u32,
    last_sent: Instant,
    last_body: String,
}

impl BackupProgressNotifier {
    pub fn should_use() -> bool {
        load_app_settings().notifications.should_notify_backup_progress()
    }

    /// Start a persistent notification; returns `None` if `notify-send` is unavailable.
    pub fn start(summary: String, initial_body: &str) -> Option<Self> {
        if !Self::should_use() {
            return None;
        }
        let id = spawn_notify_send(&summary, initial_body, None, 0, &[])?;
        Some(Self {
            summary,
            notification_id: id,
            last_sent: Instant::now(),
            last_body: initial_body.to_string(),
        })
    }

    pub fn update(&mut self, body: &str) {
        let body = body.trim();
        if body.is_empty() {
            return;
        }
        let now = Instant::now();
        if body == self.last_body && now.duration_since(self.last_sent) < PROGRESS_MIN_INTERVAL {
            return;
        }
        if now.duration_since(self.last_sent) < PROGRESS_MIN_INTERVAL {
            return;
        }
        self.last_body = body.to_string();
        self.last_sent = now;
        let hints = progress_hints(body);
        let _ = spawn_notify_send(
            &self.summary,
            body,
            Some(self.notification_id),
            0,
            &hints,
        );
    }

    pub fn finish(&mut self, summary: &str, body: &str) {
        let body = truncate_notification_body(body);
        let _ = spawn_notify_send(
            summary,
            &body,
            Some(self.notification_id),
            FINISH_EXPIRE_MS,
            &[],
        );
    }
}

fn truncate_notification_body(text: &str) -> String {
    const MAX: usize = 500;
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    let end = text
        .char_indices()
        .nth(MAX)
        .map(|(i, _)| i)
        .unwrap_or(0);
    if end == 0 {
        return text.to_string();
    }
    format!("{}…", &text[..end])
}

/// GNOME notification center progress bar hints when a percentage is present in the PBS line.
fn progress_hints(body: &str) -> Vec<(&'static str, String)> {
    let Some(percent) = parse_percent_from_progress(body) else {
        return Vec::new();
    };
    vec![
        ("int:value", percent.to_string()),
        ("int:value:max", "100".to_string()),
    ]
}

fn parse_percent_from_progress(text: &str) -> Option<u32> {
    for token in text.split_whitespace() {
        if let Some(num) = token.strip_suffix('%') {
            if let Ok(v) = num.parse::<u32>() {
                return Some(v.min(100));
            }
        }
    }
    None
}

fn spawn_notify_send(
    summary: &str,
    body: &str,
    replace_id: Option<u32>,
    expire_ms: u32,
    hints: &[(&str, String)],
) -> Option<u32> {
    let mut cmd = Command::new("notify-send");
    cmd.arg("-a")
        .arg(APP_NAME)
        .arg("-i")
        .arg(ICON)
        .arg("-t")
        .arg(expire_ms.to_string());

    if let Some(id) = replace_id {
        cmd.arg("-r").arg(id.to_string());
    } else {
        cmd.arg("-p");
    }

    for (name, value) in hints {
        cmd.arg("-h").arg(format!("{name}:{value}"));
    }

    cmd.arg(summary).arg(body);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    if replace_id.is_some() {
        return replace_id;
    }

    let id_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    id_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_percentage_token() {
        assert_eq!(
            parse_percent_from_progress("processed 42% uploaded 1 GiB"),
            Some(42)
        );
    }

    #[test]
    fn no_percent_returns_none() {
        assert_eq!(
            parse_percent_from_progress("processed 2.471 GiB uploaded 2.439 GiB"),
            None
        );
    }
}
