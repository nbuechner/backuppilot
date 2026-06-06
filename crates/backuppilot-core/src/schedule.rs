//! When scheduled backups are due (local time, one run per slot).

use chrono::{DateTime, Datelike, Local, Timelike};

use crate::profile::{BackupProfile, Schedule, ScheduleType};

/// Unique key for a schedule firing window (avoids duplicate runs in the same slot).
pub type ScheduleSlot = String;

/// Returns a slot key if `profile` should start a backup at `now`, else `None`.
pub fn due_schedule_slot(profile: &BackupProfile, now: DateTime<Local>) -> Option<ScheduleSlot> {
    if !profile.enabled {
        return None;
    }

    match profile.schedule.schedule_type {
        ScheduleType::Hourly => hourly_slot(now),
        ScheduleType::Daily => daily_slot(&profile.schedule, now),
        ScheduleType::Weekly => weekly_slot(&profile.schedule, now),
        ScheduleType::OnLogin | ScheduleType::Manual => None,
        ScheduleType::Custom => custom_slot(&profile.schedule, now),
    }
}

/// Slot key for a one-shot "on login" backup when the daemon starts.
pub fn on_login_slot(profile_id: i64) -> ScheduleSlot {
    format!("on-login-{profile_id}")
}

fn hourly_slot(now: DateTime<Local>) -> Option<ScheduleSlot> {
    // Fire once per hour (checked each minute; slot is the whole hour).
    Some(format!("hourly-{}", now.format("%Y-%m-%d-%H")))
}

fn daily_slot(schedule: &Schedule, now: DateTime<Local>) -> Option<ScheduleSlot> {
    let (hour, minute) = parse_hhmm(schedule.time.as_deref().unwrap_or("12:00"))?;
    if now.hour() != hour || now.minute() != minute {
        return None;
    }
    Some(format!("daily-{}", now.format("%Y-%m-%d")))
}

fn custom_slot(schedule: &Schedule, now: DateTime<Local>) -> Option<ScheduleSlot> {
    let expr = schedule.cron_expr.as_deref()?.trim();
    if expr.is_empty() {
        return None;
    }
    let cron = croner::Cron::new(expr)
        .with_seconds_optional()
        .with_dom_and_dow()
        .parse()
        .ok()?;
    if cron.is_time_matching(&now).ok()? {
        Some(format!("custom-{}", now.format("%Y-%m-%d-%H-%M")))
    } else {
        None
    }
}

fn weekly_slot(schedule: &Schedule, now: DateTime<Local>) -> Option<ScheduleSlot> {
    let target_weekday = u32::from(schedule.weekday.unwrap_or(1)); // Monday default
    if now.weekday().number_from_monday() != target_weekday {
        return None;
    }
    let (hour, minute) = parse_hhmm(schedule.time.as_deref().unwrap_or("12:00"))?;
    if now.hour() != hour || now.minute() != minute {
        return None;
    }
    Some(format!(
        "weekly-{}-{}",
        now.format("%Y-%m-%d"),
        target_weekday
    ))
}

/// Parses `HH:MM` (24h). Returns `None` on invalid input.
pub fn parse_hhmm(time: &str) -> Option<(u32, u32)> {
    let mut parts = time.trim().split(':');
    let hour: u32 = parts.next()?.parse().ok()?;
    let minute: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    Some((hour, minute))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Schedule;
    use chrono::{Local, TimeZone};

    fn profile_with_schedule(schedule: Schedule) -> BackupProfile {
        let now = Local::now();
        BackupProfile {
            id: 1,
            name: "test".into(),
            enabled: true,
            api_token_configured: false,
            repository: String::new(),
            namespace: None,
            backup_id: "host".into(),
            paths: vec!["/tmp".into()],
            excludes: vec![],
            schedule,
            conditions: Default::default(),
            health_check: Default::default(),
            encryption_key_id: None,
            server_fingerprint: None,
            created_at: now.into(),
            updated_at: now.into(),
        }
    }

    fn local_at(hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 5, 18, hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn parse_hhmm_valid() {
        assert_eq!(parse_hhmm("12:00"), Some((12, 0)));
        assert_eq!(parse_hhmm("09:30"), Some((9, 30)));
        assert!(parse_hhmm("25:00").is_none());
    }

    #[test]
    fn daily_due_at_configured_time() {
        let p = profile_with_schedule(Schedule {
            schedule_type: ScheduleType::Daily,
            time: Some("14:30".into()),
            weekday: None,
            cron_expr: None,
        });
        assert!(due_schedule_slot(&p, local_at(14, 29)).is_none());
        assert_eq!(
            due_schedule_slot(&p, local_at(14, 30)).as_deref(),
            Some("daily-2026-05-18")
        );
    }

    #[test]
    fn hourly_due_every_hour() {
        let p = profile_with_schedule(Schedule {
            schedule_type: ScheduleType::Hourly,
            time: None,
            weekday: None,
            cron_expr: None,
        });
        assert_eq!(
            due_schedule_slot(&p, local_at(9, 15)).as_deref(),
            Some("hourly-2026-05-18-09")
        );
    }

    #[test]
    fn custom_cron_daily_noon() {
        let p = profile_with_schedule(Schedule {
            schedule_type: ScheduleType::Custom,
            time: None,
            weekday: None,
            cron_expr: Some("0 12 * * *".into()),
        });
        assert!(due_schedule_slot(&p, local_at(11, 59)).is_none());
        assert_eq!(
            due_schedule_slot(&p, local_at(12, 0)).as_deref(),
            Some("custom-2026-05-18-12-00")
        );
    }
}
