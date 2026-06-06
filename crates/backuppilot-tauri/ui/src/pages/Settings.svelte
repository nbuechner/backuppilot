<script>
  import { getSettings, saveSettings, resetAllData, getUpdateState, checkForUpdates } from '../lib/ipc.js';
  import { onDestroy } from 'svelte';

  let settings = $state(null);
  let loading  = $state(true);
  let error    = $state('');
  let saving   = $state(false);
  let saved    = $state(false);
  let savedTimer = null;

  let updateState = $state(null);
  let checkingUpdate = $state(false);
  let confirmReset = $state(false);

  async function load() {
    try {
      [settings, updateState] = await Promise.all([getSettings(), getUpdateState()]);
      error = '';
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function save() {
    saving = true;
    try {
      await saveSettings(settings);
      saved = true;
      if (savedTimer) clearTimeout(savedTimer);
      savedTimer = setTimeout(() => { saved = false; }, 2000);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function doCheckForUpdates() {
    checkingUpdate = true;
    try {
      const result = await checkForUpdates(settings.updates.channel);
      updateState = await getUpdateState();
    } catch (e) {
      error = String(e);
    } finally {
      checkingUpdate = false;
    }
  }

  async function doResetAllData() {
    try {
      await resetAllData();
      confirmReset = false;
      alert('All data has been reset. Please restart the application.');
    } catch (e) {
      error = String(e);
    }
  }

  onDestroy(() => { if (savedTimer) clearTimeout(savedTimer); });
  load();
</script>

<div class="page-header">
  <h1 class="page-title">Settings</h1>
  <div class="header-actions">
    {#if saved}<span class="saved-badge">Saved ✓</span>{/if}
    <button class="btn-primary" onclick={save} disabled={saving || !settings}>
      {saving ? 'Saving…' : 'Save Settings'}
    </button>
  </div>
</div>

{#if error}
  <div class="error-box">{error}</div>
{/if}

{#if loading}
  <div class="center"><span class="spinner"></span></div>
{:else if settings}

  <!-- Notifications -->
  <div class="card section">
    <h2 class="section-header">Notifications</h2>
    <label class="toggle-row">
      <span>Enable notifications</span>
      <input type="checkbox" bind:checked={settings.notifications.enabled} onchange={save} />
    </label>
    {#if settings.notifications.enabled}
      <div class="sub-settings">
        <label class="toggle-row">
          <span>Notify on success</span>
          <input type="checkbox" bind:checked={settings.notifications.notify_on_success} onchange={save} />
        </label>
        <label class="toggle-row">
          <span>Notify on failure</span>
          <input type="checkbox" bind:checked={settings.notifications.notify_on_failure} onchange={save} />
        </label>
        <label class="toggle-row">
          <span>Notify on skipped</span>
          <input type="checkbox" bind:checked={settings.notifications.notify_on_skipped} onchange={save} />
        </label>
        <label class="toggle-row">
          <span>Notify on restore</span>
          <input type="checkbox" bind:checked={settings.notifications.notify_on_restore} onchange={save} />
        </label>
        <label class="toggle-row">
          <span>Health warnings</span>
          <input type="checkbox" bind:checked={settings.notifications.notify_on_health_warning} onchange={save} />
        </label>
        <label class="toggle-row">
          <span>Update available</span>
          <input type="checkbox" bind:checked={settings.notifications.notify_on_update} onchange={save} />
        </label>
        <label class="toggle-row">
          <span>Backup progress</span>
          <input type="checkbox" bind:checked={settings.notifications.notify_on_backup_progress} onchange={save} />
        </label>
      </div>
    {/if}
  </div>

  <!-- Appearance -->
  <div class="card section">
    <h2 class="section-header">Appearance</h2>
    <div class="field-row">
      <label for="color-scheme">Color scheme</label>
      <select id="color-scheme" bind:value={settings.appearance.color_scheme} onchange={save}>
        <option value="system">System</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </div>
    <div class="field-row">
      <label for="language">Language</label>
      <select id="language" bind:value={settings.appearance.language} onchange={save}>
        <option value="system">System</option>
        <option value="english">English</option>
        <option value="german">German</option>
        <option value="french">French</option>
        <option value="italian">Italian</option>
      </select>
    </div>
    <label class="toggle-row">
      <span>Advanced mode</span>
      <input type="checkbox" bind:checked={settings.appearance.advanced_mode} onchange={save} />
    </label>
  </div>

  <!-- Health Check -->
  <div class="card section">
    <h2 class="section-header">Health Check</h2>
    <p class="section-desc">Days since last successful backup before status changes.</p>
    <div class="field-row">
      <label for="warn-days">Warning after (days)</label>
      <input id="warn-days" type="number" min="1" max="365"
        bind:value={settings.health_check.warn_after_days} onchange={save} />
    </div>
    <div class="field-row">
      <label for="critical-days">Critical after (days)</label>
      <input id="critical-days" type="number" min="1" max="365"
        bind:value={settings.health_check.critical_after_days} onchange={save} />
    </div>
  </div>

  <!-- Backup Conditions -->
  <div class="card section">
    <h2 class="section-header">Backup Conditions</h2>
    <p class="section-desc">Global defaults applied to all profiles.</p>
    <label class="toggle-row">
      <span>Require AC power</span>
      <input type="checkbox" bind:checked={settings.conditions.require_ac_power} onchange={save} />
    </label>
    <label class="toggle-row">
      <span>Require server reachable</span>
      <input type="checkbox" bind:checked={settings.conditions.require_server_reachable} onchange={save} />
    </label>
  </div>

  <!-- Log Retention -->
  <div class="card section">
    <h2 class="section-header">Log Retention</h2>
    <div class="field-row">
      <label for="retain-days">Keep logs for (days)</label>
      <input id="retain-days" type="number" min="7" max="3650"
        bind:value={settings.log_retention.retain_days} onchange={save} />
    </div>
    <p class="section-desc">Set to 0 to keep all logs indefinitely. Minimum 7, maximum 3650.</p>
  </div>

  <!-- Updates -->
  <div class="card section">
    <h2 class="section-header">Updates</h2>
    <label class="toggle-row">
      <span>Check for updates automatically</span>
      <input type="checkbox" bind:checked={settings.updates.check_automatically} onchange={save} />
    </label>
    <label class="toggle-row">
      <span>Notify when update available</span>
      <input type="checkbox" bind:checked={settings.updates.notify_when_available} onchange={save} />
    </label>
    <div class="field-row">
      <label for="update-channel">Update channel</label>
      <select id="update-channel" bind:value={settings.updates.channel} onchange={save}>
        <option value="stable">Stable</option>
        <option value="beta">Beta</option>
      </select>
    </div>
    {#if updateState}
      <div class="update-status">
        {#if updateState.available_version}
          <span class="badge-update">Update available: {updateState.available_version}</span>
        {:else}
          <span class="text-muted">Up to date</span>
        {/if}
      </div>
    {/if}
    <button class="btn-ghost" onclick={doCheckForUpdates} disabled={checkingUpdate}>
      {checkingUpdate ? 'Checking…' : 'Check Now'}
    </button>
  </div>

  <!-- Reset -->
  <div class="card section danger-zone">
    <h2 class="section-header danger">Danger Zone</h2>
    {#if !confirmReset}
      <button class="btn-danger" onclick={() => confirmReset = true}>Reset All Data…</button>
      <p class="section-desc">Deletes all profiles, logs, and settings. Cannot be undone.</p>
    {:else}
      <p class="confirm-text">Are you sure? This will delete all profiles, logs, and settings.</p>
      <div class="confirm-actions">
        <button class="btn-danger" onclick={doResetAllData}>Yes, Reset Everything</button>
        <button class="btn-ghost" onclick={() => confirmReset = false}>Cancel</button>
      </div>
    {/if}
  </div>

{/if}

<style>
  .page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
  }
  .page-title { font-size: 20px; font-weight: 600; margin: 0; }
  .header-actions { display: flex; align-items: center; gap: 10px; }
  .saved-badge { font-size: 12px; color: #16a34a; font-weight: 600; }

  .center { display: flex; justify-content: center; padding: 40px; }
  .error-box {
    background: #f8d7da; color: #721c24;
    border: 1px solid #f5c6cb; border-radius: var(--radius); padding: 14px 18px;
    margin-bottom: 16px;
  }

  .section { margin-bottom: 16px; padding: 20px; }
  .section-header {
    font-size: 13px; font-weight: 700; text-transform: uppercase;
    letter-spacing: 0.06em; color: var(--text-muted); margin-bottom: 14px;
  }
  .section-header.danger { color: #dc2626; }
  .section-desc { font-size: 12px; color: var(--text-muted); margin: 6px 0 12px; }

  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 0;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    cursor: pointer;
  }
  .toggle-row:last-child { border-bottom: none; }
  .toggle-row input[type="checkbox"] { width: 16px; height: 16px; cursor: pointer; }

  .sub-settings {
    margin-left: 16px;
    border-left: 2px solid var(--border);
    padding-left: 12px;
    margin-top: 4px;
  }

  .field-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 0;
    font-size: 13px;
    border-bottom: 1px solid var(--border);
    gap: 16px;
  }
  .field-row:last-of-type { border-bottom: none; }
  .field-row label { color: var(--text); }
  .field-row select, .field-row input[type="number"] {
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 13px;
    background: var(--bg);
    color: var(--text);
    min-width: 140px;
  }

  .update-status { margin: 10px 0 6px; font-size: 13px; }
  .badge-update {
    background: #dbeafe; color: #1d4ed8;
    padding: 3px 8px; border-radius: 999px; font-size: 12px; font-weight: 600;
  }
  .text-muted { color: var(--text-muted); }

  .danger-zone { border: 1px solid #fecaca; }
  .confirm-text { font-size: 13px; color: #dc2626; margin-bottom: 12px; }
  .confirm-actions { display: flex; gap: 8px; }

  .btn-danger {
    background: #dc2626; color: #fff; border: none;
    padding: 8px 16px; border-radius: var(--radius-sm); font-size: 13px;
    font-weight: 600; cursor: pointer;
  }
  .btn-danger:hover { background: #b91c1c; }
  .btn-ghost {
    background: transparent; border: 1px solid var(--border);
    padding: 8px 16px; border-radius: var(--radius-sm); font-size: 13px;
    font-weight: 500; cursor: pointer; color: var(--text);
  }
  .btn-ghost:hover { background: var(--bg); }
</style>
