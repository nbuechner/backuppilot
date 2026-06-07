use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json;

use crate::error::{CoreError, Result};
use crate::paths::{database_path, ensure_data_dirs};
use crate::pbs_install_result::{
    consume_install_result_file, PBS_CLIENT_INSTALL_DISPLAY, PBS_CLIENT_INSTALL_KIND,
};
use crate::pbs_repository::PbsRepositoryParts;
use crate::encryption::{
    create_encryption_key_record, delete_encryption_key_files, generate_key_password,
    import_encryption_key_record, key_absolute_path, read_key_fingerprint, redact_encryption_key,
    CreateEncryptionKeyInput, EncryptionKey, ImportEncryptionKeyInput,
};
use crate::profile::{
    ActivityLogEntry, BackupConditions, BackupProfile, BackupRun, HealthCheck, NewProfile,
    RunStatus, Schedule,
};
use crate::secrets::{delete_api_token, hydrate_profile_repository, persist_profile_credentials};

/// Legacy per-profile conditions and health thresholds (database migration only).
#[derive(Debug, Clone)]
pub struct LegacyProfileSettings {
    pub conditions: BackupConditions,
    pub health_check: HealthCheck,
}

/// Result of deleting old activity log rows from the database.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActivityPruneResult {
    pub runs_deleted: u64,
    pub system_events_deleted: u64,
}

impl ActivityPruneResult {
    pub fn total_deleted(&self) -> u64 {
        self.runs_deleted.saturating_add(self.system_events_deleted)
    }
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open() -> Result<Self> {
        ensure_data_dirs().map_err(CoreError::Io)?;
        let path = database_path();
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Empty in-memory database (releases the on-disk file before a wipe).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS profiles (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT NOT NULL,
                enabled         INTEGER NOT NULL DEFAULT 1,
                repository      TEXT NOT NULL,
                namespace       TEXT,
                backup_id       TEXT NOT NULL,
                paths_json      TEXT NOT NULL DEFAULT '[]',
                excludes_json   TEXT NOT NULL DEFAULT '[]',
                schedule_json   TEXT NOT NULL,
                conditions_json TEXT NOT NULL DEFAULT '{}',
                health_json     TEXT NOT NULL DEFAULT '{}',
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS runs (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                profile_id      INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                started_at      TEXT NOT NULL,
                finished_at     TEXT,
                status          TEXT NOT NULL,
                error_message   TEXT,
                bytes_uploaded  INTEGER NOT NULL DEFAULT 0,
                snapshot_id     TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_runs_profile_started
                ON runs(profile_id, started_at DESC);

            CREATE TABLE IF NOT EXISTS system_events (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                kind            TEXT NOT NULL,
                display_name    TEXT NOT NULL,
                status          TEXT NOT NULL,
                message         TEXT,
                occurred_at     TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_system_events_occurred
                ON system_events(occurred_at DESC);

            CREATE TABLE IF NOT EXISTS scheduler_slots (
                profile_id      INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                slot_key        TEXT NOT NULL,
                fired_at        TEXT NOT NULL,
                PRIMARY KEY (profile_id, slot_key)
            );

            CREATE TABLE IF NOT EXISTS encryption_keys (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT NOT NULL UNIQUE,
                key_file        TEXT NOT NULL,
                password_hint   TEXT,
                created_at      TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS pending_retries (
                profile_id      INTEGER PRIMARY KEY REFERENCES profiles(id) ON DELETE CASCADE,
                retry_after     TEXT NOT NULL,
                reason          TEXT,
                created_at      TEXT NOT NULL
            );
            "#,
        )?;
        let _ = self.conn.execute(
            "ALTER TABLE profiles ADD COLUMN encryption_key_id INTEGER REFERENCES encryption_keys(id) ON DELETE SET NULL",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE encryption_keys ADD COLUMN last_exported_at TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE encryption_keys ADD COLUMN fingerprint TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE profiles ADD COLUMN server_fingerprint TEXT",
            [],
        );
        Ok(())
    }

    pub fn list_encryption_keys(&self) -> Result<Vec<EncryptionKey>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, name, key_file, password_hint, created_at, last_exported_at, fingerprint
               FROM encryption_keys ORDER BY name COLLATE NOCASE"#,
        )?;
        let mut keys: Vec<EncryptionKey> = stmt
            .query_map([], |row| row_to_encryption_key(row))?
            .collect::<std::result::Result<_, _>>()?;
        for key in &mut keys {
            self.ensure_encryption_key_fingerprint(key)?;
            *key = redact_encryption_key(key.clone());
        }
        Ok(keys)
    }

    pub fn get_encryption_key(&self, id: i64) -> Result<EncryptionKey> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, name, key_file, password_hint, created_at, last_exported_at, fingerprint
               FROM encryption_keys WHERE id = ?1"#,
        )?;
        let mut key = stmt
            .query_row(params![id], |row| row_to_encryption_key(row))
            .map_err(|_| CoreError::PbsCommand(format!("encryption key not found: {id}")))?;
        self.ensure_encryption_key_fingerprint(&mut key)?;
        Ok(redact_encryption_key(key))
    }

    fn ensure_encryption_key_fingerprint(&self, key: &mut EncryptionKey) -> Result<()> {
        if key.fingerprint.is_some() {
            return Ok(());
        }
        let Some(password) = crate::secrets::load_encryption_key_password(key.id)? else {
            return Ok(());
        };
        let path = key_absolute_path(&key.key_file);
        if !path.is_file() {
            return Ok(());
        }
        if let Ok(fp) = read_key_fingerprint(&path, &password) {
            self.conn.execute(
                "UPDATE encryption_keys SET fingerprint = ?1 WHERE id = ?2",
                params![fp, key.id],
            )?;
            key.fingerprint = Some(fp);
        }
        Ok(())
    }

    pub fn mark_encryption_key_exported(&self, id: i64) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE encryption_keys SET last_exported_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), id],
        )?;
        if updated == 0 {
            return Err(CoreError::PbsCommand(format!("encryption key not found: {id}")));
        }
        Ok(())
    }

    pub fn create_encryption_key(&self, input: &CreateEncryptionKeyInput) -> Result<EncryptionKey> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(CoreError::PbsCommand("key name is required".into()));
        }
        self.cleanup_stale_encryption_keys()?;
        if self.encryption_key_name_exists(name)? {
            return Err(CoreError::PbsCommand(format!(
                "an encryption key named «{name}» already exists — choose another name or delete the existing key"
            )));
        }

        // Auto-generate a password when none was supplied (e.g. from the GUI form).
        let owned;
        let input: &CreateEncryptionKeyInput = if input.password.is_empty() {
            owned = CreateEncryptionKeyInput {
                password: generate_key_password(),
                ..input.clone()
            };
            &owned
        } else {
            input
        };

        let now = Utc::now().to_rfc3339();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            r#"INSERT INTO encryption_keys (name, key_file, password_hint, created_at)
               VALUES (?1, '', NULL, ?2)"#,
            params![name, now],
        )?;
        let id = tx.last_insert_rowid();
        let key = match create_encryption_key_record(id, input) {
            Ok(key) => key,
            Err(e) => {
                let _ = tx.rollback();
                let path = key_absolute_path(&format!("encryption-keys/{id}.json"));
                if path.is_file() {
                    let _ = std::fs::remove_file(path);
                }
                return Err(e);
            }
        };
        let mut key = key;
        if let Ok(fp) = read_key_fingerprint(&key_absolute_path(&key.key_file), &input.password) {
            tx.execute(
                "UPDATE encryption_keys SET key_file = ?1, password_hint = ?2, fingerprint = ?3 WHERE id = ?4",
                params![key.key_file, key.password_hint, fp, id],
            )?;
            key.fingerprint = Some(fp);
        } else {
            tx.execute(
                "UPDATE encryption_keys SET key_file = ?1, password_hint = ?2 WHERE id = ?3",
                params![key.key_file, key.password_hint, id],
            )?;
        }
        tx.commit()?;
        Ok(redact_encryption_key(key))
    }

    pub fn import_encryption_key(&self, input: &ImportEncryptionKeyInput) -> Result<EncryptionKey> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(CoreError::PbsCommand("key name is required".into()));
        }
        self.cleanup_stale_encryption_keys()?;
        if self.encryption_key_name_exists(name)? {
            return Err(CoreError::PbsCommand(format!(
                "an encryption key named «{name}» already exists — choose another name or delete the existing key"
            )));
        }

        let now = Utc::now().to_rfc3339();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            r#"INSERT INTO encryption_keys (name, key_file, password_hint, created_at)
               VALUES (?1, '', NULL, ?2)"#,
            params![name, now],
        )?;
        let id = tx.last_insert_rowid();
        let key = match import_encryption_key_record(id, input) {
            Ok(key) => key,
            Err(e) => {
                let _ = tx.rollback();
                let path = key_absolute_path(&format!("encryption-keys/{id}.json"));
                if path.is_file() {
                    let _ = std::fs::remove_file(path);
                }
                return Err(e);
            }
        };
        let mut key = key;
        if let Ok(fp) = read_key_fingerprint(&key_absolute_path(&key.key_file), &input.password) {
            tx.execute(
                "UPDATE encryption_keys SET key_file = ?1, password_hint = ?2, fingerprint = ?3 WHERE id = ?4",
                params![key.key_file, key.password_hint, fp, id],
            )?;
            key.fingerprint = Some(fp);
        } else {
            tx.execute(
                "UPDATE encryption_keys SET key_file = ?1, password_hint = ?2 WHERE id = ?3",
                params![key.key_file, key.password_hint, id],
            )?;
        }
        tx.commit()?;
        Ok(redact_encryption_key(key))
    }

    /// Removes incomplete key rows left after a failed create/import.
    fn cleanup_stale_encryption_keys(&self) -> Result<()> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, key_file FROM encryption_keys
               WHERE key_file = '' OR key_file IS NULL"#,
        )?;
        let stale: Vec<i64> = stmt
            .query_map([], |row| row.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect();
        for id in stale {
            let profiles_using: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM profiles WHERE encryption_key_id = ?1",
                params![id],
                |row| row.get(0),
            )?;
            if profiles_using == 0 {
                let _ = delete_encryption_key_files(&EncryptionKey {
                    id,
                    name: String::new(),
                    key_file: format!("encryption-keys/{id}.json"),
                    password_hint: None,
                    created_at: Utc::now(),
                    last_exported_at: None,
                    fingerprint: None,
                    profiles_using: Vec::new(),
                    profile_usage: Vec::new(),
                    password_configured: false,
                    in_use: false,
                });
                self.conn
                    .execute("DELETE FROM encryption_keys WHERE id = ?1", params![id])?;
            }
        }
        Ok(())
    }

    fn encryption_key_name_exists(&self, name: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM encryption_keys WHERE name = ?1 COLLATE NOCASE",
            params![name],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn delete_encryption_key(&self, id: i64) -> Result<()> {
        let key = self.get_encryption_key(id)?;
        let profiles_using: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM profiles WHERE encryption_key_id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        if profiles_using > 0 {
            return Err(CoreError::PbsCommand(
                "encryption key is still assigned to one or more profiles".into(),
            ));
        }
        delete_encryption_key_files(&key)?;
        let deleted = self
            .conn
            .execute("DELETE FROM encryption_keys WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Err(CoreError::PbsCommand(format!("encryption key not found: {id}")));
        }
        Ok(())
    }

    pub fn profiles_using_encryption_key(&self, key_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM profiles WHERE encryption_key_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map(params![key_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(CoreError::from)
    }

    pub fn scheduler_slot_fired(&self, profile_id: i64, slot_key: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(
            "SELECT 1 FROM scheduler_slots WHERE profile_id = ?1 AND slot_key = ?2 LIMIT 1",
        )?;
        let found = stmt
            .query_row(params![profile_id, slot_key], |_| Ok(()))
            .optional()?
            .is_some();
        Ok(found)
    }

    pub fn record_scheduler_slot(&self, profile_id: i64, slot_key: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            r#"INSERT INTO scheduler_slots (profile_id, slot_key, fired_at)
               VALUES (?1, ?2, ?3)
               ON CONFLICT(profile_id, slot_key) DO UPDATE SET fired_at = excluded.fired_at"#,
            params![profile_id, slot_key, now],
        )?;
        Ok(())
    }

    pub fn apply_global_conditions_to_profiles(
        &self,
        conditions: &BackupConditions,
        health_check: &HealthCheck,
    ) -> Result<u32> {
        let conditions_json = serde_json::to_string(conditions)?;
        let health_json = serde_json::to_string(health_check)?;
        let updated = self.conn.execute(
            r#"UPDATE profiles
               SET conditions_json = ?1, health_json = ?2, updated_at = ?3"#,
            params![conditions_json, health_json, Utc::now().to_rfc3339()],
        )?;
        Ok(updated as u32)
    }

    /// Reads per-profile conditions and health from the database (pre-v2 migration).
    pub fn list_legacy_profile_settings(&self) -> Result<Vec<LegacyProfileSettings>> {
        let mut stmt = self
            .conn
            .prepare("SELECT conditions_json, health_json FROM profiles ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            let conditions: BackupConditions =
                serde_json::from_str(row.get::<_, String>(0)?.as_str()).unwrap_or_default();
            let health_check: HealthCheck =
                serde_json::from_str(row.get::<_, String>(1)?.as_str()).unwrap_or_default();
            Ok(LegacyProfileSettings {
                conditions,
                health_check,
            })
        })?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(CoreError::from)
    }

    pub fn list_profiles(&self) -> Result<Vec<BackupProfile>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, name, enabled, repository, namespace, backup_id,
                      paths_json, excludes_json, schedule_json, conditions_json,
                      health_json, encryption_key_id, server_fingerprint,
                      created_at, updated_at
               FROM profiles ORDER BY name"#,
        )?;
        let mut profiles = Vec::new();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let profile = row_to_profile(&row)?;
            profiles.push(self.finalize_profile(profile)?);
        }
        Ok(profiles)
    }

    pub fn find_profile_id_by_name(&self, name: &str) -> Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM profiles WHERE name = ?1 COLLATE NOCASE LIMIT 1")?;
        let mut rows = stmt.query(params![name.trim()])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(row.get(0)?));
        }
        Ok(None)
    }

    pub fn get_profile(&self, id: i64) -> Result<BackupProfile> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, name, enabled, repository, namespace, backup_id,
                      paths_json, excludes_json, schedule_json, conditions_json,
                      health_json, encryption_key_id, server_fingerprint,
                      created_at, updated_at
               FROM profiles WHERE id = ?1"#,
        )?;
        let profile = stmt
            .query_row(params![id], |row| row_to_profile(row))
            .optional()?
            .ok_or(CoreError::ProfileNotFound(id))?;
        self.finalize_profile(profile)
    }

    pub fn insert_profile(&self, new: &NewProfile) -> Result<BackupProfile> {
        let now = Utc::now();
        let schedule_json = serde_json::to_string(&new.schedule)?;
        let conditions_json = serde_json::to_string(&new.conditions)?;
        let health_json = serde_json::to_string(&new.health_check)?;
        let paths_json = serde_json::to_string(&new.paths)?;
        let excludes_json = serde_json::to_string(&new.excludes)?;
        let (repository_db, token_parts) = repository_for_storage(&new.repository)?;

        self.conn.execute(
            r#"INSERT INTO profiles (
                name, enabled, repository, namespace, backup_id,
                paths_json, excludes_json, schedule_json, conditions_json,
                health_json, encryption_key_id, server_fingerprint,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"#,
            params![
                new.name,
                i32::from(new.enabled),
                repository_db,
                new.namespace,
                new.backup_id,
                paths_json,
                excludes_json,
                schedule_json,
                conditions_json,
                health_json,
                new.encryption_key_id,
                new.server_fingerprint,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        let id = self.conn.last_insert_rowid();
        if let Some(parts) = token_parts {
            let stored = persist_profile_credentials(id, &parts)?;
            self.set_repository_raw(id, &stored)?;
        }
        self.get_profile(id)
    }

    pub fn update_profile(&self, id: i64, update: &NewProfile) -> Result<BackupProfile> {
        let now = Utc::now();
        let schedule_json = serde_json::to_string(&update.schedule)?;
        let conditions_json = serde_json::to_string(&update.conditions)?;
        let health_json = serde_json::to_string(&update.health_check)?;
        let paths_json = serde_json::to_string(&update.paths)?;
        let excludes_json = serde_json::to_string(&update.excludes)?;
        let (repository_db, token_parts) = repository_for_storage(&update.repository)?;

        let rows = self.conn.execute(
            r#"UPDATE profiles SET
                name = ?1, enabled = ?2, repository = ?3, namespace = ?4,
                backup_id = ?5, paths_json = ?6, excludes_json = ?7,
                schedule_json = ?8, conditions_json = ?9, health_json = ?10,
                encryption_key_id = ?11, server_fingerprint = ?12, updated_at = ?13
               WHERE id = ?14"#,
            params![
                update.name,
                i32::from(update.enabled),
                repository_db,
                update.namespace,
                update.backup_id,
                paths_json,
                excludes_json,
                schedule_json,
                conditions_json,
                health_json,
                update.encryption_key_id,
                update.server_fingerprint,
                now.to_rfc3339(),
                id,
            ],
        )?;
        if rows == 0 {
            return Err(CoreError::ProfileNotFound(id));
        }
        if let Some(parts) = token_parts {
            let stored = persist_profile_credentials(id, &parts)?;
            self.set_repository_raw(id, &stored)?;
        }
        self.get_profile(id)
    }

    pub fn set_profile_enabled(&self, id: i64, enabled: bool) -> Result<BackupProfile> {
        let now = Utc::now();
        let updated = self.conn.execute(
            "UPDATE profiles SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            params![i32::from(enabled), now.to_rfc3339(), id],
        )?;
        if updated == 0 {
            return Err(CoreError::ProfileNotFound(id));
        }
        self.get_profile(id)
    }

    pub fn delete_profile(&self, id: i64) -> Result<()> {
        let deleted = self
            .conn
            .execute("DELETE FROM profiles WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Err(CoreError::ProfileNotFound(id));
        }
        let _ = delete_api_token(id);
        Ok(())
    }

    fn set_repository_raw(&self, id: i64, repository: &str) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE profiles SET repository = ?1 WHERE id = ?2",
            params![repository, id],
        )?;
        if updated == 0 {
            return Err(CoreError::ProfileNotFound(id));
        }
        Ok(())
    }

    fn finalize_profile(&self, mut profile: BackupProfile) -> Result<BackupProfile> {
        if let Ok(parts) = PbsRepositoryParts::parse(&profile.repository) {
            if !parts.api_token_secret().is_empty() {
                let stored = crate::secrets::migrate_repository_tokens(profile.id, &profile.repository)?;
                self.set_repository_raw(profile.id, &stored)?;
                profile.repository = stored;
            }
        }
        profile.repository = hydrate_profile_repository(profile.id, &profile.repository)?;
        Ok(profile)
    }

    pub fn insert_run(&self, profile_id: i64, status: RunStatus) -> Result<BackupRun> {
        let started_at = Utc::now();
        self.conn.execute(
            r#"INSERT INTO runs (profile_id, started_at, status)
               VALUES (?1, ?2, ?3)"#,
            params![
                profile_id,
                started_at.to_rfc3339(),
                run_status_str(status),
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(BackupRun {
            id,
            profile_id,
            started_at,
            finished_at: None,
            status,
            error_message: None,
            bytes_uploaded: 0,
            snapshot_id: None,
        })
    }

    pub fn finish_run(
        &self,
        run_id: i64,
        status: RunStatus,
        error_message: Option<String>,
        bytes_uploaded: u64,
        snapshot_id: Option<String>,
    ) -> Result<BackupRun> {
        let finished_at = Utc::now();
        self.conn.execute(
            r#"UPDATE runs
               SET finished_at = ?1, status = ?2, error_message = ?3,
                   bytes_uploaded = ?4, snapshot_id = ?5
               WHERE id = ?6"#,
            params![
                finished_at.to_rfc3339(),
                run_status_str(status),
                error_message,
                bytes_uploaded,
                snapshot_id,
                run_id,
            ],
        )?;
        self.get_run(run_id)
    }

    pub fn get_run(&self, id: i64) -> Result<BackupRun> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, profile_id, started_at, finished_at, status,
                      error_message, bytes_uploaded, snapshot_id
               FROM runs WHERE id = ?1"#,
        )?;
        stmt.query_row(params![id], |row| row_to_run(row))
            .map_err(CoreError::from)
    }

    pub fn latest_run_for_profile(&self, profile_id: i64) -> Result<Option<BackupRun>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, profile_id, started_at, finished_at, status,
                      error_message, bytes_uploaded, snapshot_id
               FROM runs
               WHERE profile_id = ?1
               ORDER BY started_at DESC
               LIMIT 1"#,
        )?;
        stmt.query_row(params![profile_id], |row| row_to_run(row))
            .optional()
            .map_err(CoreError::from)
    }

    pub fn latest_successful_run(&self, profile_id: i64) -> Result<Option<BackupRun>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, profile_id, started_at, finished_at, status,
                      error_message, bytes_uploaded, snapshot_id
               FROM runs
               WHERE profile_id = ?1 AND status = 'success'
               ORDER BY started_at DESC
               LIMIT 1"#,
        )?;
        stmt.query_row(params![profile_id], |row| row_to_run(row))
            .optional()
            .map_err(CoreError::from)
    }

    pub fn list_runs_for_profile(&self, profile_id: i64, limit: u32) -> Result<Vec<BackupRun>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, profile_id, started_at, finished_at, status,
                      error_message, bytes_uploaded, snapshot_id
               FROM runs
               WHERE profile_id = ?1
               ORDER BY started_at DESC
               LIMIT ?2"#,
        )?;
        let rows = stmt.query_map(params![profile_id, limit], |row| row_to_run(row))?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(CoreError::from)
    }

    pub fn insert_system_event(
        &self,
        kind: &str,
        display_name: &str,
        status: RunStatus,
        message: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            r#"INSERT INTO system_events (kind, display_name, status, message, occurred_at)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
            params![
                kind,
                display_name,
                run_status_str(status),
                message,
                now.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Imports a PBS install result file from the terminal wrapper, if present.
    pub fn ingest_pbs_install_result_file(&self) -> Result<()> {
        let Some(result) = consume_install_result_file()? else {
            return Ok(());
        };
        self.insert_system_event(
            PBS_CLIENT_INSTALL_KIND,
            PBS_CLIENT_INSTALL_DISPLAY,
            result.run_status(),
            result.message.as_deref(),
        )
    }

    fn list_system_activity(&self, limit: u32) -> Result<Vec<ActivityLogEntry>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT id, kind, display_name, status, message, occurred_at
               FROM system_events
               ORDER BY occurred_at DESC
               LIMIT ?1"#,
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let status: RunStatus = parse_run_status(row.get::<_, String>(3)?.as_str());
            let at = parse_dt(row.get::<_, String>(5)?)?;
            Ok(ActivityLogEntry {
                profile_id: 0,
                profile_name: row.get(2)?,
                run: BackupRun {
                    id: row.get(0)?,
                    profile_id: 0,
                    started_at: at,
                    finished_at: Some(at),
                    status,
                    error_message: row.get(4)?,
                    bytes_uploaded: 0,
                    snapshot_id: None,
                },
                is_system: true,
                system_kind: Some(row.get(1)?),
            })
        })?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(CoreError::from)
    }

    /// Recent backup runs and system events, newest first.
    pub fn list_recent_activity(&self, limit: u32) -> Result<Vec<ActivityLogEntry>> {
        self.ingest_pbs_install_result_file()?;

        let fetch = limit.saturating_mul(2).max(limit);
        let mut entries = self.list_backup_activity(fetch)?;
        entries.append(&mut self.list_system_activity(fetch)?);
        entries.sort_by(|a, b| b.run.started_at.cmp(&a.run.started_at));
        entries.truncate(limit as usize);
        Ok(entries)
    }

    /// Removes backup runs and system events older than `cutoff` (compared via RFC3339 timestamps).
    pub fn prune_activity_before(&self, cutoff: DateTime<Utc>) -> Result<ActivityPruneResult> {
        let cutoff_str = cutoff.to_rfc3339();
        let runs_deleted = self
            .conn
            .execute("DELETE FROM runs WHERE started_at < ?1", params![cutoff_str])?
            as u64;
        let system_events_deleted = self
            .conn
            .execute(
                "DELETE FROM system_events WHERE occurred_at < ?1",
                params![cutoff_str],
            )? as u64;
        Ok(ActivityPruneResult {
            runs_deleted,
            system_events_deleted,
        })
    }

    fn list_backup_activity(&self, limit: u32) -> Result<Vec<ActivityLogEntry>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT r.id, r.profile_id, p.name, r.started_at, r.finished_at, r.status,
                      r.error_message, r.bytes_uploaded, r.snapshot_id
               FROM runs r
               INNER JOIN profiles p ON p.id = r.profile_id
               ORDER BY r.started_at DESC
               LIMIT ?1"#,
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let status: RunStatus = parse_run_status(row.get::<_, String>(5)?.as_str());
            Ok(ActivityLogEntry {
                profile_id: row.get(1)?,
                profile_name: row.get(2)?,
                run: BackupRun {
                    id: row.get(0)?,
                    profile_id: row.get(1)?,
                    started_at: parse_dt(row.get::<_, String>(3)?)?,
                    finished_at: row
                        .get::<_, Option<String>>(4)?
                        .map(|s| parse_dt(s))
                        .transpose()?,
                    status,
                    error_message: row.get(6)?,
                    bytes_uploaded: row.get::<_, i64>(7)? as u64,
                    snapshot_id: row.get(8)?,
                },
                is_system: false,
                system_kind: None,
            })
        })?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(CoreError::from)
    }

    pub fn upsert_pending_retry(
        &self,
        profile_id: i64,
        retry_after: DateTime<Utc>,
        reason: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            r#"INSERT INTO pending_retries (profile_id, retry_after, reason, created_at)
               VALUES (?1, ?2, ?3, ?4)
               ON CONFLICT(profile_id) DO UPDATE SET
                 retry_after = excluded.retry_after,
                 reason = excluded.reason,
                 created_at = excluded.created_at"#,
            params![
                profile_id,
                retry_after.to_rfc3339(),
                reason,
                now.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn clear_pending_retry(&self, profile_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM pending_retries WHERE profile_id = ?1", params![profile_id])?;
        Ok(())
    }

    pub fn list_due_pending_retries(&self, now: DateTime<Utc>) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT profile_id FROM pending_retries WHERE retry_after <= ?1 ORDER BY retry_after",
        )?;
        let rows = stmt.query_map(params![now.to_rfc3339()], |row| row.get::<_, i64>(0))?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(CoreError::from)
    }
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<BackupProfile> {
    let paths: Vec<String> = serde_json::from_str(row.get::<_, String>(6)?.as_str())
        .unwrap_or_default();
    let excludes: Vec<String> = serde_json::from_str(row.get::<_, String>(7)?.as_str())
        .unwrap_or_default();
    let schedule: Schedule = serde_json::from_str(row.get::<_, String>(8)?.as_str())
        .unwrap_or_default();
    let conditions: BackupConditions =
        serde_json::from_str(row.get::<_, String>(9)?.as_str()).unwrap_or_default();
    let health_check: HealthCheck =
        serde_json::from_str(row.get::<_, String>(10)?.as_str()).unwrap_or_default();
    Ok(BackupProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        enabled: row.get::<_, i32>(2)? != 0,
        api_token_configured: false,
        repository: row.get(3)?,
        namespace: row.get(4)?,
        backup_id: row.get(5)?,
        paths,
        excludes,
        schedule,
        conditions,
        health_check,
        encryption_key_id: row.get(11)?,
        server_fingerprint: row.get(12)?,
        created_at: parse_dt(row.get::<_, String>(13)?)?,
        updated_at: parse_dt(row.get::<_, String>(14)?)?,
    })
}

fn row_to_encryption_key(row: &rusqlite::Row<'_>) -> rusqlite::Result<EncryptionKey> {
    Ok(EncryptionKey {
        id: row.get(0)?,
        name: row.get(1)?,
        key_file: row.get(2)?,
        password_hint: row.get(3)?,
        created_at: parse_dt(row.get::<_, String>(4)?)?,
        last_exported_at: row
            .get::<_, Option<String>>(5)?
            .and_then(|s| parse_dt(s).ok()),
        fingerprint: row.get::<_, Option<String>>(6)?,
        profiles_using: Vec::new(),
        profile_usage: Vec::new(),
        password_configured: false,
        in_use: false,
    })
}

fn repository_for_storage(
    repository: &str,
) -> Result<(String, Option<PbsRepositoryParts>)> {
    let parts = PbsRepositoryParts::parse(repository).map_err(|e| CoreError::PbsCommand(e.to_string()))?;
    if parts.api_token_secret().is_empty() {
        return Ok((repository.to_string(), None));
    }
    let mut stripped = parts.clone();
    let (token_id, _) = parts.api_token_parts();
    stripped.token = token_id; // keep name, drop secret (secret goes to keyring)
    Ok((
        crate::pbs_repository::encode_repository(&stripped),
        Some(parts),
    ))
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<BackupRun> {
    let status: RunStatus = parse_run_status(row.get::<_, String>(4)?.as_str());
    Ok(BackupRun {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        started_at: parse_dt(row.get::<_, String>(2)?)?,
        finished_at: row
            .get::<_, Option<String>>(3)?
            .map(|s| parse_dt(s))
            .transpose()?,
        status,
        error_message: row.get(5)?,
        bytes_uploaded: row.get::<_, i64>(6)? as u64,
        snapshot_id: row.get(7)?,
    })
}

fn parse_dt(s: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

fn run_status_str(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Pending => "pending",
        RunStatus::Running => "running",
        RunStatus::Success => "success",
        RunStatus::Failed => "failed",
        RunStatus::Skipped => "skipped",
        RunStatus::Cancelled => "cancelled",
    }
}

fn parse_run_status(s: &str) -> RunStatus {
    match s {
        "running" => RunStatus::Running,
        "success" => RunStatus::Success,
        "failed" => RunStatus::Failed,
        "skipped" => RunStatus::Skipped,
        "cancelled" => RunStatus::Cancelled,
        _ => RunStatus::Pending,
    }
}
