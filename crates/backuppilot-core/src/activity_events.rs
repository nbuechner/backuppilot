//! System-event kinds shown in the activity log (see `system_events` table).

pub const SNAPSHOT_MOUNT_KIND: &str = "snapshot_mount";
pub const SNAPSHOT_UNMOUNT_KIND: &str = "snapshot_unmount";
pub const RESTORE_STARTED_KIND: &str = "restore_started";
pub const RESTORE_FINISHED_KIND: &str = "restore_finished";
