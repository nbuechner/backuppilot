<script>
  import { listRecentActivity } from '../lib/ipc.js';

  let entries  = $state([]);
  let loading  = $state(true);
  let error    = $state('');

  async function load() {
    try {
      entries = await listRecentActivity(50);
      error = '';
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function levelClass(level) {
    if (level === 'Error')   return 'level-error';
    if (level === 'Warning') return 'level-warn';
    return 'level-info';
  }

  load();
  const interval = setInterval(load, 10000);
  import { onDestroy } from 'svelte';
  onDestroy(() => clearInterval(interval));
</script>

<h1 class="page-title">Recent Activity</h1>

{#if loading}
  <div class="center"><span class="spinner"></span></div>
{:else if error}
  <div class="error-box">{error}</div>
{:else if entries.length === 0}
  <div class="empty">No activity yet.</div>
{:else}
  <div class="log card">
    {#each entries as e}
      <div class="log-row {levelClass(e.level)}">
        <span class="log-time">{new Date(e.timestamp * 1000).toLocaleString()}</span>
        <span class="log-profile">{e.profile_name ?? '—'}</span>
        <span class="log-msg">{e.message}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .page-title { font-size: 20px; font-weight: 600; margin-bottom: 20px; }
  .center { display: flex; justify-content: center; padding: 40px; }
  .error-box {
    background: #f8d7da; color: #721c24;
    border: 1px solid #f5c6cb; border-radius: var(--radius); padding: 14px 18px;
  }
  .empty { color: var(--text-muted); padding: 40px 0; text-align: center; }

  .log { padding: 0; overflow: hidden; }

  .log-row {
    display: grid;
    grid-template-columns: 160px 120px 1fr;
    gap: 12px;
    padding: 9px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    align-items: center;
  }
  .log-row:last-child { border-bottom: none; }

  .level-error { background: #fff5f5; }
  .level-warn  { background: #fffbf0; }

  .log-time    { color: var(--text-muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .log-profile { font-weight: 500; }
  .log-msg     { color: var(--text); }
</style>
