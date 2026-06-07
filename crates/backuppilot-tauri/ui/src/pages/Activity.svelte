<script>
  import { listRecentActivity, listProfiles } from '../lib/ipc.js';
  import { onDestroy } from 'svelte';

  let entries   = $state(null);
  let profiles  = $state(null);
  let error     = $state('');
  let selected  = $state(null);

  let loading = $derived(entries === null && profiles === null && !error);

  // Filters
  let filterProfile = $state('all');
  let filterStatus  = $state('all');
  let filterSearch  = $state('');

  async function load() {
    const timeout = new Promise((_, reject) =>
      setTimeout(() => reject(new Error('Activity load timed out — daemon may be unresponsive')), 20_000)
    );
    try {
      [entries, profiles] = await Promise.race([
        Promise.all([listRecentActivity(200), listProfiles()]),
        timeout,
      ]);
      error = '';
    } catch (e) {
      error = String(e);
    }
  }

  function statusClass(status) {
    const s = status?.toLowerCase();
    if (s === 'failed' || s === 'cancelled') return 'level-error';
    if (s === 'skipped') return 'level-warn';
    return 'level-info';
  }

  function statusBadgeClass(status) {
    const s = status?.toLowerCase();
    if (s === 'success') return 'badge badge-ok';
    if (s === 'failed')  return 'badge badge-error';
    if (s === 'running') return 'badge badge-running';
    return 'badge badge-idle';
  }

  function formatDate(iso) {
    if (!iso) return '—';
    return new Date(iso).toLocaleString();
  }

  function formatBytes(bytes) {
    if (!bytes) return '—';
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
    if (bytes < 1024 * 1024 * 1024) return (bytes / 1024 / 1024).toFixed(1) + ' MB';
    return (bytes / 1024 / 1024 / 1024).toFixed(2) + ' GB';
  }

  let filtered = $derived((entries ?? []).filter(e => {
    if (filterProfile !== 'all' && String(e.profile_id) !== filterProfile) return false;
    if (filterStatus !== 'all' && e.run.status !== filterStatus) return false;
    if (filterSearch) {
      const q = filterSearch.toLowerCase();
      if (!e.profile_name?.toLowerCase().includes(q) &&
          !e.run.error_message?.toLowerCase().includes(q) &&
          !e.run.snapshot_id?.toLowerCase().includes(q)) return false;
    }
    return true;
  }));

  load();
  const interval = setInterval(load, 15000);
  onDestroy(() => clearInterval(interval));
</script>

<div class="page-header">
  <h1 class="page-title">Recent Activity</h1>
</div>

{#if error}
  <div class="error-box">{error}</div>
{/if}

<div class="filters card">
  <select bind:value={filterProfile}>
    <option value="all">All profiles</option>
    {#each (profiles ?? []) as p}
      <option value={String(p.id)}>{p.name}</option>
    {/each}
  </select>

  <select bind:value={filterStatus}>
    <option value="all">All statuses</option>
    <option value="Success">Success</option>
    <option value="Failed">Failed</option>
    <option value="Skipped">Skipped</option>
    <option value="Running">Running</option>
    <option value="Cancelled">Cancelled</option>
  </select>

  <input class="search-input" type="text" placeholder="Search…" bind:value={filterSearch} />
</div>

{#if loading}
  <div class="center"><span class="spinner"></span></div>
{:else if filtered.length === 0}
  <div class="empty">No activity entries match the current filters.</div>
{:else}
  <div class="log card">
    {#each filtered as e (e.run.id)}
      <button class="log-row {statusClass(e.run.status)}" onclick={() => selected = selected?.run.id === e.run.id ? null : e}>
        <span class="log-time">{formatDate(e.run.started_at)}</span>
        <span class="log-profile">{e.profile_name ?? '—'}</span>
        <span class={statusBadgeClass(e.run.status)}>{e.run.status}</span>
        <span class="log-msg">{e.run.error_message ?? (e.run.snapshot_id ? 'Snapshot: ' + e.run.snapshot_id : '')}</span>
      </button>

      {#if selected?.run.id === e.run.id}
        <div class="detail-panel">
          <div class="detail-grid">
            <span class="detail-label">Profile</span>
            <span>{e.profile_name}</span>

            <span class="detail-label">Status</span>
            <span class={statusBadgeClass(e.run.status)}>{e.run.status}</span>

            <span class="detail-label">Started</span>
            <span>{formatDate(e.run.started_at)}</span>

            <span class="detail-label">Finished</span>
            <span>{formatDate(e.run.finished_at)}</span>

            {#if e.run.snapshot_id}
              <span class="detail-label">Snapshot</span>
              <span class="mono">{e.run.snapshot_id}</span>
            {/if}

            <span class="detail-label">Uploaded</span>
            <span>{formatBytes(e.run.bytes_uploaded)}</span>

            {#if e.run.error_message}
              <span class="detail-label">Error</span>
              <span class="error-msg">{e.run.error_message}</span>
            {/if}
          </div>
        </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .page-header { margin-bottom: 16px; }
  .page-title { font-size: 20px; font-weight: 600; margin: 0 0 16px; }

  .filters {
    display: flex;
    gap: 10px;
    padding: 12px 16px;
    margin-bottom: 12px;
    align-items: center;
    flex-wrap: wrap;
  }
  .filters select, .search-input {
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 13px;
    background: var(--bg);
    color: var(--text);
  }
  .search-input { flex: 1; min-width: 160px; }

  .center { display: flex; justify-content: center; padding: 40px; }
  .error-box {
    background: #f8d7da; color: #721c24;
    border: 1px solid #f5c6cb; border-radius: var(--radius); padding: 14px 18px;
    margin-bottom: 16px;
  }
  .empty { color: var(--text-muted); padding: 40px 0; text-align: center; }

  .log { padding: 0; overflow: hidden; }

  .log-row {
    display: grid;
    grid-template-columns: 155px 130px 80px 1fr;
    gap: 10px;
    padding: 9px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    align-items: center;
    width: 100%;
    background: transparent;
    border-left: none;
    border-right: none;
    border-top: none;
    text-align: left;
    cursor: pointer;
    border-radius: 0;
  }
  .log-row:last-child { border-bottom: none; }
  .log-row:hover { background: #f0f4ff; }

  .level-error { background: #fff5f5; }
  .level-error:hover { background: #fee2e2; }
  .level-warn  { background: #fffbf0; }
  .level-warn:hover  { background: #fef3c7; }

  .log-time    { color: var(--text-muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .log-profile { font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .log-msg     { color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }

  .badge { padding: 2px 7px; border-radius: 999px; font-size: 11px; font-weight: 600; white-space: nowrap; }
  .badge-ok      { background: #dcfce7; color: #166534; }
  .badge-error   { background: #fee2e2; color: #991b1b; }
  .badge-running { background: #dbeafe; color: #1e40af; }
  .badge-idle    { background: #f1f5f9; color: #475569; }

  .detail-panel {
    background: #f8faff;
    border-bottom: 1px solid var(--border);
    padding: 14px 20px;
  }
  .detail-grid {
    display: grid;
    grid-template-columns: 90px 1fr;
    gap: 6px 16px;
    font-size: 13px;
    align-items: baseline;
  }
  .detail-label { color: var(--text-muted); font-size: 12px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; }
  .error-msg { color: #dc2626; }
  .mono { font-family: monospace; font-size: 12px; }
</style>
