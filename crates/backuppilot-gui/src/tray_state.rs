//! Aggregated tray icon / tooltip state from daemon profile statuses.

use backuppilot_core::profile::{HealthState, ProfileStatus};
use backuppilot_i18n::{tr, tr_fmt};

use crate::icons::{self, TrayIconAssets};
use crate::window::{in_progress_status_text, run_status_label};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayIndicator {
    #[default]
    Unknown,
    Ok,
    Warning,
    Critical,
    Running,
}

#[derive(Debug, Clone)]
pub struct TrayProfileLine {
    pub id: i64,
    pub name: String,
    pub backup_in_progress: bool,
}

#[derive(Debug)]
pub struct TrayState {
    pub indicator: TrayIndicator,
    pub tooltip_body: String,
    pub spin_frame: u8,
    pub profiles: Vec<TrayProfileLine>,
    pub icon_assets: TrayIconAssets,
}

impl Default for TrayState {
    fn default() -> Self {
        Self {
            indicator: TrayIndicator::default(),
            tooltip_body: String::new(),
            spin_frame: 0,
            profiles: Vec::new(),
            icon_assets: icons::tray_assets_for_indicator(TrayIndicator::Unknown, 0),
        }
    }
}

impl TrayState {
    pub fn update_from_statuses(&mut self, statuses: &[ProfileStatus]) {
        self.profiles = statuses
            .iter()
            .map(|s| TrayProfileLine {
                id: s.profile_id,
                name: s.profile_name.clone(),
                backup_in_progress: s.backup_in_progress,
            })
            .collect();

        let new_indicator = aggregate_indicator(statuses);
        let indicator_changed = new_indicator != self.indicator;
        self.indicator = new_indicator;
        self.tooltip_body = build_tooltip(statuses);

        if indicator_changed {
            self.spin_frame = 0;
            self.icon_assets =
                icons::tray_assets_for_indicator(self.indicator, self.spin_frame);
        } else {
            self.icon_assets.overlay_icon_name =
                icons::overlay_icon_for_indicator(self.indicator, self.spin_frame);
        }
    }

    /// Tray must stay visible — `Passive` hides the icon on GNOME when everything is OK.
    pub fn ksni_status(&self) -> ksni::Status {
        use ksni::Status;
        match self.indicator {
            TrayIndicator::Critical => Status::NeedsAttention,
            _ => Status::Active,
        }
    }
}

fn aggregate_indicator(statuses: &[ProfileStatus]) -> TrayIndicator {
    if statuses.is_empty() {
        return TrayIndicator::Unknown;
    }
    if statuses.iter().any(|s| s.backup_in_progress) {
        return TrayIndicator::Running;
    }

    let mut worst = HealthState::Ok;
    for status in statuses {
        worst = match (worst, status.health) {
            (HealthState::Critical, _) | (_, HealthState::Critical) => HealthState::Critical,
            (HealthState::Warning, _) | (_, HealthState::Warning) => HealthState::Warning,
            (HealthState::Unknown, _) | (_, HealthState::Unknown) => HealthState::Unknown,
            _ => HealthState::Ok,
        };
    }

    match worst {
        HealthState::Critical => TrayIndicator::Critical,
        HealthState::Warning => TrayIndicator::Warning,
        HealthState::Ok => TrayIndicator::Ok,
        HealthState::Unknown => TrayIndicator::Unknown,
    }
}

fn build_tooltip(statuses: &[ProfileStatus]) -> String {
    if statuses.is_empty() {
        return tr("No backup profiles configured yet.");
    }

    statuses
        .iter()
        .map(profile_tooltip_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn profile_tooltip_line(status: &ProfileStatus) -> String {
    if status.backup_in_progress {
        return tr_fmt(
            "{name}: {detail}",
            &[
                ("name", &status.profile_name),
                ("detail", &in_progress_status_text(status)),
            ],
        );
    }

    let health = match status.health {
        HealthState::Ok => tr("OK"),
        HealthState::Warning => tr("Warning"),
        HealthState::Critical => tr("Critical"),
        HealthState::Unknown => tr("Unknown"),
    };

    let run = if let Some(run) = &status.last_run {
        let label = run_status_label(run);
        if let Some(msg) = &run.error_message {
            tr_fmt("Last run: {label} , {msg}", &[("label", &label), ("msg", msg)])
        } else {
            tr_fmt("Last run: {label}", &[("label", &label)])
        }
    } else {
        tr("No backup run yet")};

    tr_fmt(
        "{name} ({health}): {run}",
        &[
            ("name", &status.profile_name),
            ("health", &health),
            ("run", &run),
        ],
    )
}
