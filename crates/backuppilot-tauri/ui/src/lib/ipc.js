import { invoke } from '@tauri-apps/api/core';

export async function ipcCall(method, params = null) {
  return invoke('ipc_call', { method, params });
}

// Profiles
export const listProfiles        = ()                      => ipcCall('list_profiles');
export const getProfile          = (profileId)             => ipcCall('get_profile',    { profile_id: profileId });
export const listStatuses        = ()                      => ipcCall('list_statuses');
export const createProfile       = (profileJson)           => ipcCall('create_profile', { profile_json: JSON.stringify(profileJson) });
export const updateProfile       = (profileId, profileJson) => ipcCall('update_profile', { profile_id: profileId, profile_json: JSON.stringify(profileJson) });
export const deleteProfile       = (profileId)             => ipcCall('delete_profile', { profile_id: profileId });
export const setProfileEnabled   = (profileId, enabled)    => ipcCall('set_profile_enabled', { profile_id: profileId, enabled });

// Backup control
export const runBackup           = (profileId)  => ipcCall('run_backup',              { profile_id: profileId });
export const cancelBackup        = (profileId)  => ipcCall('cancel_backup',           { profile_id: profileId });
export const cancelAllBackups    = ()           => ipcCall('cancel_all_running_backups');
export const pauseAllBackups     = ()           => ipcCall('pause_all_backups');
export const togglePauseAll      = ()           => ipcCall('toggle_pause_all_backups');

// Activity
export const listRecentActivity  = (limit)      => ipcCall('list_recent_activity',    { limit });
export const listRunsForProfile  = (profileId, limit) => ipcCall('list_runs_for_profile', { profile_id: profileId, limit });

// Snapshots & restore
export const listSnapshots       = (profileId)  => ipcCall('list_snapshots',          { profile_id: profileId });
export const listCatalog         = (req)        => ipcCall('list_catalog',            { request_json: JSON.stringify(req) });
export const restoreArchive      = (req)        => ipcCall('restore_archive',         { request_json: JSON.stringify(req) });
export const listActiveMounts    = ()           => ipcCall('list_active_mounts');
export const mountSnapshot       = (req)        => ipcCall('mount_snapshot',          { request_json: JSON.stringify(req) });
export const unmountSnapshot     = (mountId)    => ipcCall('unmount_snapshot',        { mount_id: mountId });

// Encryption keys
export const listEncryptionKeys  = ()           => ipcCall('list_encryption_keys');
export const createEncryptionKey = (input)      => ipcCall('create_encryption_key',   { input_json: JSON.stringify(input) });
export const importEncryptionKey = (input)      => ipcCall('import_encryption_key',   { input_json: JSON.stringify(input) });
export const deleteEncryptionKey = (keyId)      => ipcCall('delete_encryption_key',   { key_id: keyId });
export const markKeyExported     = (keyId)      => ipcCall('mark_encryption_key_exported', { key_id: keyId });
export const profilesUsingKey    = (keyId)      => ipcCall('profiles_using_encryption_key', { key_id: keyId });

// Settings
export const getSettings         = ()           => ipcCall('get_settings');
export const saveSettings        = (settings)   => ipcCall('save_settings', { settings_json: JSON.stringify(settings) });

// Updates
export const getUpdateState      = ()           => ipcCall('get_update_state');
export const checkForUpdates     = (channel)    => ipcCall('check_for_updates', { channel });
export const dismissUpdate       = ()           => ipcCall('dismiss_available_update');

// Preflight / credentials
export const runPreflight        = (profileId)  => ipcCall('run_preflight',           { profile_id: profileId });
export const verifyCredentials   = (input)      => ipcCall('verify_profile_credentials', { input_json: JSON.stringify(input) });

// Misc
export const resetAllData        = ()           => ipcCall('reset_all_data');
export const getVersion          = ()           => ipcCall('version');
