<script>
  import { listEncryptionKeys, createEncryptionKey, deleteEncryptionKey, markKeyExported, profilesUsingKey } from '../lib/ipc.js';

  let keys    = $state([]);
  let loading = $state(true);
  let error   = $state('');

  // Create form
  let showCreate  = $state(false);
  let newKeyName  = $state('');
  let creating    = $state(false);

  // Delete confirmation
  let pendingDelete = $state(null);   // key object
  let deleteUsage   = $state([]);

  async function load() {
    try {
      keys = await listEncryptionKeys();
      error = '';
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function doCreate() {
    if (!newKeyName.trim()) return;
    creating = true;
    try {
      await createEncryptionKey({ name: newKeyName.trim() });
      newKeyName = '';
      showCreate = false;
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      creating = false;
    }
  }

  async function confirmDelete(key) {
    pendingDelete = key;
    try {
      deleteUsage = await profilesUsingKey(key.id);
    } catch {
      deleteUsage = [];
    }
  }

  async function doDelete() {
    if (!pendingDelete) return;
    try {
      await deleteEncryptionKey(pendingDelete.id);
      pendingDelete = null;
      deleteUsage = [];
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  async function doMarkExported(keyId) {
    try {
      await markKeyExported(keyId);
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  function formatDate(iso) {
    if (!iso) return '—';
    return new Date(iso).toLocaleDateString();
  }

  load();
</script>

<div class="page-header">
  <h1 class="page-title">Encryption Keys</h1>
  <button class="btn-primary" onclick={() => showCreate = !showCreate}>+ New Key</button>
</div>

{#if error}
  <div class="error-box">{error}</div>
{/if}

{#if showCreate}
  <div class="card create-form">
    <h2 class="section-title">Create Encryption Key</h2>
    <div class="field-row">
      <label for="key-name">Key name</label>
      <input id="key-name" type="text" placeholder="My Backup Key" bind:value={newKeyName}
        onkeydown={e => e.key === 'Enter' && doCreate()} />
    </div>
    <div class="form-actions">
      <button class="btn-primary" onclick={doCreate} disabled={creating || !newKeyName.trim()}>
        {creating ? 'Creating…' : 'Create Key'}
      </button>
      <button class="btn-ghost" onclick={() => { showCreate = false; newKeyName = ''; }}>Cancel</button>
    </div>
  </div>
{/if}

<div class="warning-callout card">
  ⚠ Keep your encryption keys safe. If you lose a key, the encrypted backups protected by that key cannot be restored.
</div>

{#if loading}
  <div class="center"><span class="spinner"></span></div>
{:else if keys.length === 0}
  <div class="empty">No encryption keys. Create one to use encrypted backups.</div>
{:else}
  <div class="keys-list card">
    {#each keys as key}
      <div class="key-row">
        <div class="key-info">
          <span class="key-name">{key.name}</span>
          {#if key.fingerprint}
            <span class="key-fp mono">{key.fingerprint}</span>
          {/if}
          <span class="key-meta">Created {formatDate(key.created_at)}</span>
        </div>
        <div class="key-actions">
          {#if !key.exported}
            <button class="btn-ghost-sm warning" onclick={() => doMarkExported(key.id)}
              title="Mark as backed up externally">
              ⚠ Not exported
            </button>
          {:else}
            <span class="exported-badge">✓ Exported</span>
          {/if}
          <button class="btn-danger-sm" onclick={() => confirmDelete(key)}>Delete</button>
        </div>
      </div>
    {/each}
  </div>
{/if}

<!-- Delete confirmation dialog -->
{#if pendingDelete}
  <div class="overlay" role="dialog" aria-modal="true">
    <div class="dialog card">
      <h2 class="dialog-title">Delete Encryption Key?</h2>
      <p>Delete <strong>{pendingDelete.name}</strong>?</p>
      {#if deleteUsage.length > 0}
        <p class="warn-text">
          This key is used by {deleteUsage.length} profile(s):
          {deleteUsage.join(', ')}. Those backups will no longer be accessible.
        </p>
      {/if}
      <p class="warn-text">This cannot be undone.</p>
      <div class="dialog-actions">
        <button class="btn-danger" onclick={doDelete}>Delete Key</button>
        <button class="btn-ghost" onclick={() => { pendingDelete = null; deleteUsage = []; }}>Cancel</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .page-header {
    display: flex; align-items: center; justify-content: space-between; margin-bottom: 20px;
  }
  .page-title { font-size: 20px; font-weight: 600; margin: 0; }

  .error-box {
    background: #f8d7da; color: #721c24;
    border: 1px solid #f5c6cb; border-radius: var(--radius); padding: 14px 18px; margin-bottom: 16px;
  }

  .create-form { padding: 20px; margin-bottom: 16px; }
  .section-title {
    font-size: 13px; font-weight: 700; text-transform: uppercase;
    letter-spacing: 0.05em; color: var(--text-muted); margin-bottom: 14px;
  }
  .field-row {
    display: flex; align-items: center; gap: 12px; margin-bottom: 14px; font-size: 13px;
  }
  .field-row label { min-width: 80px; }
  .field-row input {
    flex: 1; padding: 7px 10px; border: 1px solid var(--border);
    border-radius: var(--radius-sm); font-size: 13px; background: var(--bg);
  }
  .form-actions { display: flex; gap: 8px; }

  .warning-callout {
    background: #fffbf0; border: 1px solid #fde68a; padding: 12px 16px;
    font-size: 13px; color: #92400e; margin-bottom: 16px;
  }

  .center { display: flex; justify-content: center; padding: 40px; }
  .empty { color: var(--text-muted); padding: 40px 0; text-align: center; }

  .keys-list { padding: 0; overflow: hidden; }
  .key-row {
    display: flex; align-items: center; justify-content: space-between;
    padding: 14px 20px; border-bottom: 1px solid var(--border); gap: 16px;
  }
  .key-row:last-child { border-bottom: none; }

  .key-info { display: flex; flex-direction: column; gap: 3px; }
  .key-name { font-weight: 600; font-size: 14px; }
  .key-fp { font-family: monospace; font-size: 11px; color: var(--text-muted); }
  .key-meta { font-size: 12px; color: var(--text-muted); }
  .mono { font-family: monospace; }

  .key-actions { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }

  .exported-badge { font-size: 12px; color: #16a34a; font-weight: 600; }

  .btn-ghost {
    background: transparent; border: 1px solid var(--border);
    padding: 6px 14px; border-radius: var(--radius-sm); font-size: 13px;
    font-weight: 500; cursor: pointer; color: var(--text);
  }
  .btn-ghost:hover { background: var(--bg); }

  .btn-ghost-sm {
    background: transparent; border: 1px solid var(--border);
    padding: 4px 10px; border-radius: var(--radius-sm); font-size: 12px;
    cursor: pointer; color: var(--text);
  }
  .btn-ghost-sm.warning { color: #d97706; border-color: #fde68a; }

  .btn-danger-sm {
    background: transparent; border: 1px solid #fca5a5;
    padding: 4px 10px; border-radius: var(--radius-sm); font-size: 12px;
    cursor: pointer; color: #dc2626;
  }
  .btn-danger-sm:hover { background: #fee2e2; }

  /* Dialog */
  .overlay {
    position: fixed; inset: 0; background: rgba(0,0,0,0.4);
    display: flex; align-items: center; justify-content: center; z-index: 100;
  }
  .dialog { padding: 28px 32px; max-width: 420px; width: 90%; }
  .dialog-title { font-size: 16px; font-weight: 700; margin-bottom: 12px; }
  .dialog p { font-size: 13px; margin-bottom: 8px; }
  .warn-text { color: #dc2626; }
  .dialog-actions { display: flex; gap: 8px; margin-top: 16px; }

  .btn-danger {
    background: #dc2626; color: #fff; border: none;
    padding: 8px 18px; border-radius: var(--radius-sm); font-size: 13px;
    font-weight: 600; cursor: pointer;
  }
  .btn-danger:hover { background: #b91c1c; }
</style>
