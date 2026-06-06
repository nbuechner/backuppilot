import { invoke } from '@tauri-apps/api/core';

export async function ipcCall(method, params = null) {
  return invoke('ipc_call', { method, params });
}

export const listProfiles        = ()           => ipcCall('list_profiles');
export const listStatuses        = ()           => ipcCall('list_statuses');
export const listRecentActivity  = (limit)      => ipcCall('list_recent_activity', { limit });
export const runBackup           = (profileId)  => ipcCall('run_backup',    { profile_id: profileId });
export const cancelBackup        = (profileId)  => ipcCall('cancel_backup', { profile_id: profileId });
export const deleteProfile       = (profileId)  => ipcCall('delete_profile',{ profile_id: profileId });
export const listSnapshots       = (profileId)  => ipcCall('list_snapshots', { profile_id: profileId });
export const listCatalog         = (req)        => ipcCall('list_catalog',   { request_json: JSON.stringify(req) });
export const restoreArchive      = (req)        => ipcCall('restore_archive',{ request_json: JSON.stringify(req) });
