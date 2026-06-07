<script>
  import { listProfiles, listSnapshots, listCatalog, restoreArchive,
           listActiveMounts, mountSnapshot, unmountSnapshot,
           checkFuseAvailable, installFuse3 } from '../lib/ipc.js';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  import { onDestroy } from 'svelte';

  async function pickTargetDir() {
    const dir = await openDialog({ directory: true, multiple: false, title: 'Select restore destination' });
    if (dir) targetDir = dir;
  }

  let profiles    = $state([]);
  let snapshots   = $state([]);
  let catalog     = $state(null);   // ListCatalogResponse
  let allArchives = $state([]);     // cumulative archive list — survives per-archive reloads
  let activeMounts = $state([]);

  let selectedProfile  = $state(null);
  let selectedSnapshot = $state(null);
  let selectedArchive  = $state('');
  let currentPath      = $state('/');

  let targetDir     = $state('');
  let overwrite     = $state(false);
  let selectedPaths = $state(new Set());

  let loadingSnapshots = $state(false);
  let loadingCatalog   = $state(false);
  let restoring        = $state(false);
  let restoreResult    = $state(null);
  let mounting         = $state(false);
  let mountResult      = $state(null);
  let installingFuse3  = $state(false);
  let fuseCheck        = $state(null); // FuseCheckResult
  let error            = $state('');

  async function loadProfiles() {
    try {
      [profiles, activeMounts] = await Promise.all([listProfiles(), listActiveMounts()]);
    } catch (e) {
      error = String(e);
    }
  }

  async function selectProfile(p) {
    selectedProfile = p;
    selectedSnapshot = null;
    catalog = null;
    selectedArchive = '';
    currentPath = '/';
    snapshots = [];
    loadingSnapshots = true;
    try {
      snapshots = await listSnapshots(p.id);
    } catch (e) {
      error = String(e);
    } finally {
      loadingSnapshots = false;
    }
  }

  async function selectSnapshot(s) {
    selectedSnapshot = s;
    catalog = null;
    allArchives = [];
    selectedArchive = '';
    currentPath = '/';
    await loadCatalog('/');
  }

  async function loadCatalog(path) {
    if (!selectedProfile || !selectedSnapshot) return;
    loadingCatalog = true;
    try {
      catalog = await listCatalog({
        profile_id: selectedProfile.id,
        snapshot: selectedSnapshot.path,
        archive_name: selectedArchive || '',
        parent_path: path,
        force_refresh: false,
        encryption_key_id: null,
      });
      currentPath = path;
      if (!selectedArchive && catalog.suggested_archive) {
        selectedArchive = catalog.suggested_archive;
      }
      // Keep the largest seen archive list — a per-archive reload may return fewer
      if ((catalog.archives?.length ?? 0) > allArchives.length) {
        allArchives = catalog.archives;
      }
    } catch (e) {
      error = String(e);
    } finally {
      loadingCatalog = false;
    }
  }

  async function selectArchive(archive) {
    selectedArchive = archive;
    currentPath = '/';
    await loadCatalog('/');
  }

  async function navigateDir(path) {
    selectedPaths = new Set();
    await loadCatalog(path);
  }

  function togglePath(path) {
    const next = new Set(selectedPaths);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    selectedPaths = next;
  }

  function breadcrumbs() {
    const parts = currentPath.split('/').filter(Boolean);
    const crumbs = [{ label: '/', path: '/' }];
    let acc = '';
    for (const p of parts) {
      acc += '/' + p;
      crumbs.push({ label: p, path: acc });
    }
    return crumbs;
  }

  async function doRestore() {
    if (!selectedProfile || !selectedSnapshot || !targetDir.trim()) return;
    restoring = true;
    restoreResult = null;
    try {
      const req = {
        profile_id: selectedProfile.id,
        snapshot: selectedSnapshot.path,
        archive_name: selectedArchive,
        target_dir: targetDir,
        overwrite,
        patterns: selectedPaths.size > 0 ? Array.from(selectedPaths) : [],
        encryption_key_id: null,
      };
      restoreResult = await restoreArchive(req);
    } catch (e) {
      error = String(e);
    } finally {
      restoring = false;
    }
  }

  async function doUnmount(mountId) {
    try {
      await unmountSnapshot(mountId);
      activeMounts = await listActiveMounts();
    } catch (e) {
      error = String(e);
    }
  }

  async function doMount() {
    if (!selectedProfile || !selectedSnapshot || !selectedArchive) return;
    mounting = true;
    mountResult = null;
    try {
      const result = await mountSnapshot({
        profile_id: selectedProfile.id,
        snapshot: selectedSnapshot.path,
        archive_name: selectedArchive,
        source_label: selectedArchive,
        encryption_key_id: null,
      });
      mountResult = result;
      if (result.ok) {
        activeMounts = await listActiveMounts();
      }
    } catch (e) {
      error = String(e);
    } finally {
      mounting = false;
    }
  }

  async function doInstallFuse3() {
    installingFuse3 = true;
    try {
      const result = await installFuse3();
      if (result.ok) {
        fuseCheck = await checkFuseAvailable();
        mountResult = null;
      } else {
        error = result.message ?? 'fuse3 install failed.';
      }
    } catch (e) {
      error = String(e);
    } finally {
      installingFuse3 = false;
    }
  }

  function openInExplorer(path) {
    invoke('open_in_explorer', { path });
  }

  async function loadFuseCheck() {
    try {
      fuseCheck = await checkFuseAvailable();
    } catch {
      fuseCheck = null;
    }
  }

  function formatSnapshotDate(s) {
    // path = "host/{backup_id}/{ISO8601}"
    const ts = s.path?.split('/')[2];
    if (!ts) return '—';
    return new Date(ts).toLocaleString();
  }

  function snapshotLabel(s) {
    const parts = s.path?.split('/') ?? [];
    return parts[1] ?? s.path ?? '?';
  }

  loadProfiles();
  loadFuseCheck();
  const interval = setInterval(() => listActiveMounts().then(m => activeMounts = m).catch(() => {}), 10000);
  onDestroy(() => clearInterval(interval));
</script>

<h1 class="page-title">Restore</h1>

{#if error}
  <div class="error-box" role="alert">{error}<button class="close-btn" onclick={() => error = ''}>×</button></div>
{/if}

{#if activeMounts.length > 0}
  <div class="mount-banner card">
    <strong>Active mounts:</strong>
    {#each activeMounts as m}
      <div class="mount-row">
        <span class="mono">{m.windows_path ?? m.mount_point}</span>
        <span class="muted">{m.profile_name} / {m.archive_name}</span>
        {#if m.windows_path}
          <button class="btn-ghost-sm" onclick={() => openInExplorer(m.windows_path)}>Open</button>
        {/if}
        <button class="btn-ghost-sm" onclick={() => doUnmount(m.id)}>Unmount</button>
      </div>
    {/each}
  </div>
{/if}

<div class="restore-layout">

  <!-- Step 1: Profile -->
  <div class="panel card">
    <h2 class="panel-title">1. Profile</h2>
    {#if profiles.length === 0}
      <p class="muted">No profiles.</p>
    {:else}
      {#each profiles as p}
        <button
          class="list-item"
          class:selected={selectedProfile?.id === p.id}
          onclick={() => selectProfile(p)}
        >{p.name}</button>
      {/each}
    {/if}
  </div>

  <!-- Step 2: Snapshot -->
  <div class="panel card">
    <h2 class="panel-title">2. Snapshot</h2>
    {#if !selectedProfile}
      <p class="muted">Select a profile first.</p>
    {:else if loadingSnapshots}
      <div class="center"><span class="spinner"></span></div>
    {:else if snapshots.length === 0}
      <p class="muted">No snapshots found.</p>
    {:else}
      {#each snapshots as s}
        <button
          class="list-item snapshot-item"
          class:selected={selectedSnapshot?.path === s.path}
          onclick={() => selectSnapshot(s)}
        >
          <span class="mono">{snapshotLabel(s)}</span>
          <span class="muted">{formatSnapshotDate(s)}</span>
        </button>
      {/each}
    {/if}
  </div>

  <!-- Step 3: Archive & file browser -->
  <div class="panel card browser-panel">
    <h2 class="panel-title">3. Browse &amp; Select Files</h2>

    {#if !selectedSnapshot}
      <p class="muted">Select a snapshot first.</p>
    {:else if loadingCatalog && !catalog}
      <div class="center"><span class="spinner"></span></div>
    {:else if catalog}
      {#if allArchives.length > 1}
        <div class="archive-tabs">
          {#each allArchives as a}
            <button
              class="tab"
              class:tab-active={selectedArchive === a}
              onclick={() => selectArchive(a)}
            >{a}</button>
          {/each}
        </div>
      {/if}

      <div class="breadcrumbs">
        {#each breadcrumbs() as crumb, i}
          {#if i > 0}<span class="crumb-sep">/</span>{/if}
          <button class="crumb" onclick={() => navigateDir(crumb.path)}>{crumb.label}</button>
        {/each}
      </div>

      {#if loadingCatalog}
        <div class="center"><span class="spinner"></span></div>
      {:else if catalog.entries?.length === 0}
        <p class="muted">Empty directory.</p>
      {:else}
        <div class="file-list">
          {#if currentPath !== '/'}
            <button class="file-row" onclick={() => {
              const parts = currentPath.split('/').filter(Boolean);
              parts.pop();
              navigateDir(parts.length ? '/' + parts.join('/') : '/');
            }}>
              <span class="file-icon">📁</span>
              <span>..</span>
            </button>
          {/if}
          {#each catalog.entries as entry}
            <div class="file-row">
              {#if !entry.is_dir}
                <input type="checkbox"
                  checked={selectedPaths.has(entry.path)}
                  onchange={() => togglePath(entry.path)} />
              {:else}
                <span style="width:16px"></span>
              {/if}
              <button
                class="file-name"
                onclick={() => entry.is_dir ? navigateDir(entry.path) : togglePath(entry.path)}
              >
                <span class="file-icon">{entry.is_dir ? '📁' : '📄'}</span>
                {entry.name}
              </button>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

</div>

<!-- Step 4: Restore / Mount options — always below the 3-column grid -->
{#if selectedSnapshot}
  <div class="card restore-options">
    <h2 class="panel-title">4. Restore or Mount</h2>

    {#if selectedPaths.size > 0}
      <p class="muted">{selectedPaths.size} file(s) selected. Leave empty to restore entire archive.</p>
    {:else}
      <p class="muted">No files selected — entire archive will be restored.</p>
    {/if}

    <div class="field-row">
      <label for="target-dir">Target directory</label>
      <input id="target-dir" type="text" placeholder="C:\Users\…\restored" bind:value={targetDir} />
      <button class="btn-ghost-sm" onclick={pickTargetDir}>Browse…</button>
    </div>

    <label class="toggle-row">
      <span>Overwrite existing files</span>
      <input type="checkbox" bind:checked={overwrite} />
    </label>

    <div class="action-row">
      <button class="btn-primary" onclick={doRestore}
        disabled={restoring || !targetDir.trim()}>
        {restoring ? 'Restoring…' : 'Start Restore'}
      </button>

      <div class="mount-action">
        {#if fuseCheck === null}
          <button class="btn-ghost" disabled>Mount (checking…)</button>
        {:else if !fuseCheck.available}
          <button class="btn-ghost"
            title={fuseCheck.reason ?? 'FUSE not available'}
            disabled={!fuseCheck.can_install || !selectedArchive || installingFuse3}
            onclick={fuseCheck.can_install ? doInstallFuse3 : undefined}>
            {installingFuse3 ? 'Installing fuse3…' : (fuseCheck.can_install ? 'Install fuse3 & Mount' : 'Mount (not supported)')}
          </button>
          {#if fuseCheck.reason && !fuseCheck.can_install}
            <span class="fuse-reason">{fuseCheck.reason}</span>
          {/if}
        {:else}
          <button class="btn-ghost" onclick={doMount}
            disabled={mounting || !selectedArchive}>
            {mounting ? 'Mounting…' : 'Mount read-only'}
          </button>
        {/if}
      </div>
    </div>

    {#if restoreResult}
      {#if restoreResult.ok}
        <div class="result-ok">✓ Restore completed successfully.</div>
      {:else}
        <div class="result-error">
          Restore failed: {restoreResult.message ?? 'Unknown error'}
          {#if restoreResult.conflicts?.length > 0}
            <ul>{#each restoreResult.conflicts as c}<li class="mono">{c}</li>{/each}</ul>
          {/if}
        </div>
      {/if}
    {/if}

    {#if mountResult}
      {#if mountResult.ok}
        <div class="result-ok">
          ✓ Mounted at
          <span class="mono">{mountResult.mount?.windows_path ?? mountResult.mount?.mount_point ?? 'unknown path'}</span>
          {#if mountResult.mount?.windows_path}
            <button class="btn-inline-ok" onclick={() => openInExplorer(mountResult.mount.windows_path)}>Open in Explorer</button>
          {/if}
        </div>
      {:else if mountResult.needs_fuse3}
        <div class="result-error">
          fuse3 is required for mounting.
          {#if fuseCheck?.can_install}
            <button class="btn-inline" onclick={doInstallFuse3} disabled={installingFuse3}>
              {installingFuse3 ? 'Installing…' : 'Install fuse3'}
            </button>
          {/if}
        </div>
      {:else}
        <div class="result-error">Mount failed: {mountResult.message ?? 'Unknown error'}</div>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .page-title { font-size: 20px; font-weight: 600; margin-bottom: 16px; }

  .error-box {
    background: #f8d7da; color: #721c24;
    border: 1px solid #f5c6cb; border-radius: var(--radius); padding: 12px 16px;
    margin-bottom: 16px; display: flex; justify-content: space-between; align-items: center;
  }
  .close-btn { background: none; border: none; color: #721c24; font-size: 18px; cursor: pointer; }

  .mount-banner { padding: 12px 16px; margin-bottom: 16px; background: #fffbf0; border: 1px solid #fde68a; }
  .mount-row { display: flex; align-items: center; gap: 12px; margin-top: 6px; font-size: 13px; }

  .restore-layout {
    display: grid;
    grid-template-columns: 180px 220px 1fr;
    gap: 16px;
    margin-bottom: 16px;
    align-items: start;
  }

  .panel {
    padding: 16px;
    max-height: calc(100vh - 220px);
    overflow-y: auto;
  }
  .panel-title {
    font-size: 12px; font-weight: 700; text-transform: uppercase;
    letter-spacing: 0.06em; color: var(--text-muted); margin-bottom: 12px;
  }

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

  .snapshot-item { font-size: 12px; }

  .center { display: flex; justify-content: center; padding: 20px; }
  .muted { color: var(--text-muted); font-size: 13px; }
  .mono { font-family: monospace; font-size: 12px; }

  .browser-panel { padding: 16px; }

  .archive-tabs { display: flex; gap: 4px; margin-bottom: 12px; }
  .tab {
    padding: 5px 12px; border-radius: var(--radius-sm); border: 1px solid var(--border);
    background: transparent; font-size: 12px; cursor: pointer; color: var(--text);
  }
  .tab-active { background: #dbeafe; color: #1d4ed8; border-color: #93c5fd; }

  .breadcrumbs {
    display: flex; align-items: center; gap: 2px;
    background: var(--bg); padding: 6px 10px; border-radius: var(--radius-sm);
    margin-bottom: 10px; flex-wrap: wrap;
  }
  .crumb {
    background: none; border: none; color: #2563eb; font-size: 13px;
    cursor: pointer; padding: 1px 3px; border-radius: 3px;
  }
  .crumb:hover { background: #dbeafe; }
  .crumb-sep { color: var(--text-muted); font-size: 13px; }

  .file-list { border: 1px solid var(--border); border-radius: var(--radius-sm); overflow: hidden; }
  .file-row {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 12px; border-bottom: 1px solid var(--border); font-size: 13px;
  }
  .file-row:last-child { border-bottom: none; }
  .file-icon { font-size: 14px; }
  .file-name {
    background: none; border: none; text-align: left; cursor: pointer;
    color: var(--text); font-size: 13px; flex: 1; display: flex; align-items: center; gap: 6px;
  }
  .file-name:hover { color: #2563eb; }

  .restore-options { padding: 16px; }
  .field-row {
    display: flex; align-items: center; gap: 12px;
    padding: 10px 0; border-bottom: 1px solid var(--border); font-size: 13px;
  }
  .field-row label { color: var(--text); white-space: nowrap; min-width: 120px; }
  .field-row input[type="text"] {
    flex: 1; padding: 6px 10px; border: 1px solid var(--border);
    border-radius: var(--radius-sm); font-size: 13px; background: var(--bg); color: var(--text);
  }
  .toggle-row {
    display: flex; align-items: center; justify-content: space-between;
    padding: 10px 0; font-size: 13px; cursor: pointer;
  }
  .action-row { display: flex; align-items: center; gap: 12px; margin-top: 14px; flex-wrap: wrap; }
  .mount-action { display: flex; align-items: center; gap: 8px; }
  .fuse-reason { font-size: 12px; color: var(--text-muted); }
  .result-ok { margin-top: 12px; color: #166534; background: #dcfce7; padding: 10px 14px; border-radius: var(--radius-sm); font-size: 13px; }
  .result-error {
    margin-top: 12px; color: #991b1b; background: #fee2e2; padding: 10px 14px;
    border-radius: var(--radius-sm); font-size: 13px;
    display: flex; align-items: center; gap: 10px; flex-wrap: wrap;
  }
  .result-error ul { margin: 6px 0 0; padding-left: 16px; }
  .result-error li { font-family: monospace; font-size: 12px; }
  .btn-inline {
    background: transparent; border: 1px solid #991b1b; color: #991b1b;
    padding: 3px 10px; border-radius: var(--radius-sm); font-size: 12px;
    cursor: pointer;
  }
  .btn-inline:hover { background: #fee2e2; }
  .btn-inline-ok {
    background: transparent; border: 1px solid #166534; color: #166534;
    padding: 3px 10px; border-radius: var(--radius-sm); font-size: 12px;
    cursor: pointer; margin-left: 6px;
  }
  .btn-inline-ok:hover { background: #dcfce7; }

  .btn-ghost-sm {
    background: transparent; border: 1px solid var(--border);
    padding: 3px 10px; border-radius: var(--radius-sm); font-size: 12px;
    cursor: pointer; color: var(--text);
  }
  .btn-ghost {
    background: transparent; border: 1px solid var(--border);
    padding: 8px 18px; border-radius: var(--radius-sm); font-size: 13px;
    font-weight: 500; cursor: pointer; color: var(--text);
  }
  .btn-ghost:hover:not(:disabled) { background: var(--bg); }
  .btn-ghost:disabled { opacity: 0.5; cursor: default; }
</style>
