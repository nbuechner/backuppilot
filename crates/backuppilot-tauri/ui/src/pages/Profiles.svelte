<script>
  import { listProfiles, listStatuses, runBackup, cancelBackup, deleteProfile, getProfile } from '../lib/ipc.js';
  import { onDestroy } from 'svelte';

  let { onOpenForm, reloadKey = 0 } = $props();

  let profiles = $state([]);
  let statuses = $state([]);
  let loading  = $state(true);
  let error    = $state('');
  let busy     = $state({});

  // Trigger an immediate reload when the profile form saves (App.svelte increments reloadKey).
  $effect(() => {
    if (reloadKey > 0) load();
  });

  async function load() {
    try {
      [profiles, statuses] = await Promise.all([listProfiles(), listStatuses()]);
      error = '';
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function statusFor(id) {
    return statuses.find(s => s.profile_id === id) ?? null;
  }

  function badgeClass(st) {
    if (!st) return 'badge-idle';
    if (st.backup_in_progress)           return 'badge-running';
    if (st.last_run?.status === 'Success') return 'badge-ok';
    if (st.last_run?.status === 'Failed')  return 'badge-error';
    return 'badge-idle';
  }

  function badgeLabel(st) {
    if (!st)                               return 'Never run';
    if (st.backup_in_progress)             return 'Running…';
    if (st.last_run?.status === 'Success') return 'OK';
    if (st.last_run?.status === 'Failed')  return 'Failed';
    return 'Idle';
  }

  async function doBackup(id) {
    busy = { ...busy, [id]: true };
    try {
      await runBackup(id);
      await load();
    } catch (e) {
      alert('Backup failed: ' + e);
    } finally {
      busy = { ...busy, [id]: false };
    }
  }

  async function doCancel(id) {
    busy = { ...busy, [id]: true };
    try {
      await cancelBackup(id);
      await load();
    } finally {
      busy = { ...busy, [id]: false };
    }
  }

  async function doDelete(id, name) {
    if (!confirm(`Delete profile "${name}"?`)) return;
    await deleteProfile(id);
    await load();
  }

  async function doEdit(id) {
    try {
      onOpenForm(await getProfile(id));
    } catch (e) {
      error = String(e);
    }
  }

  load();
  const interval = setInterval(load, 5000);
  onDestroy(() => clearInterval(interval));
</script>

<div class="page-header">
  <h1 class="page-title">Backup Profiles</h1>
  <button class="btn-primary" onclick={() => onOpenForm(null)}>+ Add Profile</button>
</div>

{#if loading}
  <div class="center"><span class="spinner"></span></div>
{:else if error}
  <div class="error-box">{error}</div>
{:else if profiles.length === 0}
  <div class="empty">No profiles configured. Add a profile to get started.</div>
{:else}
  <div class="profile-list">
    {#each profiles as p}
      {@const st = statusFor(p.id)}
      {@const repo = (() => { try { return JSON.parse(p.repository); } catch { return {}; } })()}
      <div class="card profile-card">
        <div class="profile-info">
          <span class="profile-name">{p.name}</span>
          <span class="profile-host">{repo.host ?? p.repository}</span>
          {#if st?.progress_message}
            <span class="progress-msg">{st.progress_message}</span>
          {/if}
        </div>
        <div class="profile-meta">
          <span class="badge {badgeClass(st)}">{badgeLabel(st)}</span>
          {#if st?.last_run?.started_at}
            <span class="last-run">{new Date(st.last_run.started_at).toLocaleString()}</span>
          {/if}
        </div>
        <div class="profile-actions">
          {#if st?.backup_in_progress}
            <button class="btn-danger" disabled={busy[p.id]} onclick={() => doCancel(p.id)}>
              Cancel
            </button>
          {:else}
            <button class="btn-primary" disabled={busy[p.id]} onclick={() => doBackup(p.id)}>
              {busy[p.id] ? 'Starting…' : 'Backup Now'}
            </button>
          {/if}
          <button class="btn-ghost" onclick={() => doEdit(p.id)}>Edit</button>
          <button class="btn-ghost danger" onclick={() => doDelete(p.id, p.name)}>Delete</button>
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
  }
  .page-title { font-size: 20px; font-weight: 600; }

  .center { display: flex; justify-content: center; padding: 40px; }

  .error-box {
    background: #f8d7da; color: #721c24;
    border: 1px solid #f5c6cb;
    border-radius: var(--radius);
    padding: 14px 18px;
  }

  .empty {
    color: var(--text-muted);
    padding: 40px 0;
    text-align: center;
  }

  .profile-list { display: flex; flex-direction: column; gap: 12px; }

  .profile-card {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .profile-info { flex: 1; min-width: 0; }
  .profile-name { display: block; font-weight: 600; font-size: 14px; }
  .profile-host { display: block; font-size: 12px; color: var(--text-muted); margin-top: 2px; }
  .progress-msg { display: block; font-size: 12px; color: #1d4ed8; margin-top: 2px; }

  .profile-meta {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
    min-width: 120px;
  }
  .last-run { font-size: 11px; color: var(--text-muted); }

  .profile-actions { display: flex; gap: 8px; }

  .btn-ghost.danger { color: var(--red); }
  .btn-ghost.danger:hover { background: #fee2e2; }
</style>
