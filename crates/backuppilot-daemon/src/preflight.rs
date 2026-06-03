//! Daemon re-exports core preflight checks.

pub use backuppilot_core::preflight::{
    backups_globally_paused, run_preflight, PreflightOptions, PreflightReport,
};
