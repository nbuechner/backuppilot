//! Desktop notifications.
//!
//! Linux: spawns `notify-send` (freedesktop / GNOME).
//! Windows: WinRT toast notifications via `notify-rust`.
//!
//! The `BackupProgressNotifier` provides live progress updates on Linux (via
//! notification replace-by-id). On Windows only start/finish notifications are
//! shown — WinRT toasts cannot be silently replaced mid-stream.

use std::time::{Duration, Instant};

use crate::app_settings::load_app_settings;

const APP_NAME: &str = "BackupPilot";
const PROGRESS_MIN_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(not(windows))]
const FINISH_EXPIRE_MS: u32 = 10_000;

// ── Public surface ────────────────────────────────────────────────────────────

/// Shows a one-shot desktop notification.
pub fn send_desktop_notification(summary: &str, body: &str) {
    platform::send_notification(summary, body, true);
}

/// Tracks a live backup progress notification (updates the same notification).
pub struct BackupProgressNotifier {
    summary: String,
    /// On Linux: the notification replace-id returned by `notify-send -p`.
    /// On Windows: unused (always 0).
    notification_id: u32,
    last_sent: Instant,
    last_body: String,
}

impl BackupProgressNotifier {
    pub fn should_use() -> bool {
        load_app_settings().notifications.should_notify_backup_progress()
    }

    /// Starts a persistent progress notification.  Returns `None` when
    /// notifications are disabled or the notification subsystem is unavailable.
    pub fn start(summary: String, initial_body: &str) -> Option<Self> {
        if !Self::should_use() {
            return None;
        }
        let id = platform::start_progress_notification(&summary, initial_body)?;
        Some(Self {
            summary,
            notification_id: id,
            last_sent: Instant::now(),
            last_body: initial_body.to_string(),
        })
    }

    pub fn update(&mut self, body: &str) {
        let body = body.trim();
        if body.is_empty() || body == self.last_body {
            return;
        }
        let now = Instant::now();
        if now.duration_since(self.last_sent) < PROGRESS_MIN_INTERVAL {
            return;
        }
        self.last_body = body.to_string();
        self.last_sent = now;
        platform::update_progress_notification(self.notification_id, &self.summary, body);
    }

    pub fn finish(&mut self, summary: &str, body: &str) {
        let body = truncate_notification_body(body);
        platform::finish_progress_notification(self.notification_id, summary, &body);
    }
}

fn truncate_notification_body(text: &str) -> String {
    const MAX: usize = 500;
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    let end = text.char_indices().nth(MAX).map(|(i, _)| i).unwrap_or(0);
    if end == 0 {
        return text.to_string();
    }
    format!("{}…", &text[..end])
}

// ── Linux implementation (notify-send subprocess) ────────────────────────────

#[cfg(not(windows))]
mod platform {
    use std::process::{Command, Stdio};
    use crate::ids::ICON_NAME as ICON;
    use super::{APP_NAME, FINISH_EXPIRE_MS};

    pub(super) fn send_notification(summary: &str, body: &str, _expire: bool) {
        let _ = spawn_notify_send(summary, body, None, FINISH_EXPIRE_MS, &[]);
    }

    pub(super) fn start_progress_notification(summary: &str, body: &str) -> Option<u32> {
        spawn_notify_send(summary, body, None, 0, &[])
    }

    pub(super) fn update_progress_notification(id: u32, summary: &str, body: &str) {
        let hints = progress_hints(body);
        let _ = spawn_notify_send(summary, body, Some(id), 0, &hints);
    }

    pub(super) fn finish_progress_notification(id: u32, summary: &str, body: &str) {
        let _ = spawn_notify_send(summary, body, Some(id), FINISH_EXPIRE_MS, &[]);
    }

    fn progress_hints(body: &str) -> Vec<(&'static str, String)> {
        let Some(percent) = parse_percent(body) else {
            return Vec::new();
        };
        vec![
            ("int:value", percent.to_string()),
            ("int:value:max", "100".to_string()),
        ]
    }

    fn parse_percent(text: &str) -> Option<u32> {
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
        cmd.arg("-a").arg(APP_NAME)
            .arg("-i").arg(ICON)
            .arg("-t").arg(expire_ms.to_string());

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
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }
}

// ── Windows implementation (WinRT via notify-rust) ───────────────────────────

#[cfg(windows)]
mod platform {
    use super::APP_NAME;

    pub(super) fn send_notification(summary: &str, body: &str, _expire: bool) {
        show_toast(summary, body);
    }

    /// Windows toasts cannot be silently replaced by ID, so progress updates
    /// are suppressed — only the initial and final notifications are shown.
    pub(super) fn start_progress_notification(summary: &str, body: &str) -> Option<u32> {
        show_toast(summary, body);
        Some(0) // non-None means "notifier is active"
    }

    /// No-op on Windows: mid-stream toast replacement is not supported.
    pub(super) fn update_progress_notification(_id: u32, _summary: &str, _body: &str) {}

    pub(super) fn finish_progress_notification(_id: u32, summary: &str, body: &str) {
        show_toast(summary, body);
    }

    fn show_toast(summary: &str, body: &str) {
        let _ = notify_rust::Notification::new()
            .appname(APP_NAME)
            .summary(summary)
            .body(body)
            .show();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn truncates_long_body() {
        let long = "x".repeat(600);
        let truncated = super::truncate_notification_body(&long);
        assert!(truncated.chars().count() <= 504); // 500 + "…"
    }

    #[test]
    fn short_body_unchanged() {
        let body = "processed 50% uploaded 1 GiB";
        assert_eq!(super::truncate_notification_body(body), body);
    }
}
