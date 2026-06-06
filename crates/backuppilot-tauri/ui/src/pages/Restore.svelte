<script>
  import { listProfiles, listSnapshots } from '../lib/ipc.js';

  let profiles   = $state([]);
  let snapshots  = $state([]);
  let selectedProfile = $state(null);
  let loading    = $state(false);
  let error      = $state('');

  async function loadProfiles() {
    try {
      profiles = await listProfiles();
    } catch (e) {
      error = String(e);
    }
  }

  async function loadSnapshots(profileId) {
    loading = true;
    snapshots = [];
    try {
      snapshots = await listSnapshots(profileId);
      error = '';
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function selectProfile(p) {
    selectedProfile = p;
    loadSnapshots(p.id);
  }

  loadProfiles();
</script>

<h1 class="page-title">Restore</h1>

<div class="restore-layout">
  <div class="panel card">
    <h2 class="panel-title">1. Select Profile</h2>
    {#each profiles as p}
      <button
        class="list-item"
        class:selected={selectedProfile?.id === p.id}
        onclick={() => selectProfile(p)}
      >{p.name}</button>
    {/each}
    {#if profiles.length === 0}
      <p class="muted">No profiles.</p>
    {/if}
  </div>

  <div class="panel card">
    <h2 class="panel-title">2. Select Snapshot</h2>
    {#if !selectedProfile}
      <p class="muted">Select a profile first.</p>
    {:else if loading}
      <div class="center"><span class="spinner"></span></div>
    {:else if error}
      <p class="error-text">{error}</p>
    {:else if snapshots.length === 0}
      <p class="muted">No snapshots found.</p>
    {:else}
      {#each snapshots as s}
        <div class="list-item snapshot-item">
          <span>{s.backup_id}</span>
          <span class="muted">{new Date(s.backup_time * 1000).toLocaleDateString()}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .page-title { font-size: 20px; font-weight: 600; margin-bottom: 20px; }

  .restore-layout {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }

  .panel { padding: 16px; }
  .panel-title { font-size: 13px; font-weight: 600; text-transform: uppercase;
    letter-spacing: 0.05em; color: var(--text-muted); margin-bottom: 12px; }

  .list-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: transparent;
    border: none;
    text-align: left;
    font-size: 13px;
    color: var(--text);
    cursor: pointer;
  }
  .list-item:hover { background: var(--bg); }
  .list-item.selected { background: #dbeafe; color: #1d4ed8; }

  .snapshot-item { cursor: default; }

  .center { display: flex; justify-content: center; padding: 20px; }
  .muted { color: var(--text-muted); font-size: 13px; }
  .error-text { color: var(--red); font-size: 13px; }
</style>
