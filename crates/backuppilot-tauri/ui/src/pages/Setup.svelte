<script>
  import { checkWindowsSetup, installFuse3 } from '../lib/ipc.js';

  let status     = $state(null);
  let loading    = $state(true);
  let error      = $state('');
  let copied     = $state('');
  let installingFuse = $state(false);

  async function load() {
    loading = true;
    error = '';
    try {
      status = await checkWindowsSetup();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function doInstallFuse() {
    installingFuse = true;
    try {
      const result = await installFuse3();
      if (!result.ok) error = result.message ?? 'Install failed.';
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      installingFuse = false;
    }
  }

  function copyText(text, key) {
    navigator.clipboard.writeText(text).then(() => {
      copied = key;
      setTimeout(() => { if (copied === key) copied = ''; }, 2000);
    });
  }

  load();

  // Commands shown to the user
  const CMD_ENABLE_WSL = `# Run in PowerShell as Administrator:\nwsl --install\n# Then restart Windows`;
  const CMD_INSTALL_PBS = `# Run these inside a WSL terminal (Ubuntu):\nwget -O /etc/apt/trusted.gpg.d/proxmox-release-bookworm.gpg \\\n  https://enterprise.proxmox.com/debian/proxmox-release-bookworm.gpg\n\necho "deb http://download.proxmox.com/debian/pbs-client bookworm main" | \\\n  sudo tee /etc/apt/sources.list.d/pbs-client.list\n\nsudo apt update && sudo apt install -y proxmox-backup-client`;
  const CMD_INSTALL_FUSE = `# Run inside a WSL terminal:\nsudo apt install -y fuse3`;
</script>

<h1 class="page-title">Windows Setup Guide</h1>
<p class="intro">BackupPilot on Windows uses WSL (Windows Subsystem for Linux) to run
  <code>proxmox-backup-client</code>. Follow the steps below to get everything working.</p>

{#if error}
  <div class="error-box">{error}<button class="close-btn" onclick={() => error = ''}>×</button></div>
{/if}

{#if loading}
  <div class="center"><span class="spinner"></span></div>
{:else if status && !status.applicable}
  <div class="card section na-banner">
    This setup guide only applies to Windows. On Linux, install
    <code>proxmox-backup-client</code> and <code>fuse3</code> via your package manager.
  </div>
{:else if status}

  <!-- Step 1: WSL -->
  <div class="card step" class:step-ok={status.wsl_available} class:step-err={!status.wsl_available}>
    <div class="step-header">
      <span class="step-num">1</span>
      <span class="step-title">WSL (Windows Subsystem for Linux)</span>
      {#if status.wsl_available}
        <span class="badge-ok">Available</span>
      {:else}
        <span class="badge-err">Not installed</span>
      {/if}
    </div>
    {#if !status.wsl_available}
      <p class="step-desc">WSL is required to run <code>proxmox-backup-client</code>.
        Open PowerShell as Administrator and run:</p>
      <div class="cmd-block">
        <pre>{CMD_ENABLE_WSL}</pre>
        <button class="copy-btn" onclick={() => copyText('wsl --install', 'wsl')}>
          {copied === 'wsl' ? 'Copied!' : 'Copy'}
        </button>
      </div>
      <p class="step-note">After installation, restart Windows and reopen BackupPilot.</p>
    {:else}
      <p class="step-desc step-ok-text">WSL is installed and running.</p>
    {/if}
  </div>

  <!-- Step 2: proxmox-backup-client -->
  <div class="card step"
    class:step-ok={status.pbs_client_available}
    class:step-err={!status.pbs_client_available}
    class:step-disabled={!status.wsl_available}>
    <div class="step-header">
      <span class="step-num">2</span>
      <span class="step-title">proxmox-backup-client</span>
      {#if status.pbs_client_available}
        <span class="badge-ok">{status.pbs_client_version ?? 'Available'}</span>
      {:else if !status.wsl_available}
        <span class="badge-skip">Requires WSL first</span>
      {:else}
        <span class="badge-err">Not installed</span>
      {/if}
    </div>
    {#if !status.pbs_client_available && status.wsl_available}
      <p class="step-desc">Install the Proxmox Backup client inside WSL.
        Open a WSL terminal and run:</p>
      <div class="cmd-block">
        <pre>{CMD_INSTALL_PBS}</pre>
        <button class="copy-btn"
          onclick={() => copyText(
            'wget -O /etc/apt/trusted.gpg.d/proxmox-release-bookworm.gpg https://enterprise.proxmox.com/debian/proxmox-release-bookworm.gpg\necho "deb http://download.proxmox.com/debian/pbs-client bookworm main" | sudo tee /etc/apt/sources.list.d/pbs-client.list\nsudo apt update && sudo apt install -y proxmox-backup-client',
            'pbs')}>
          {copied === 'pbs' ? 'Copied!' : 'Copy'}
        </button>
      </div>
    {:else if status.pbs_client_available}
      <p class="step-desc step-ok-text">proxmox-backup-client is installed in WSL.</p>
    {/if}
  </div>

  <!-- Step 3: fuse3 -->
  <div class="card step"
    class:step-ok={status.fuse_available}
    class:step-err={!status.fuse_available && status.wsl_available}
    class:step-disabled={!status.wsl_available}>
    <div class="step-header">
      <span class="step-num">3</span>
      <span class="step-title">fuse3 (for read-only mounting)</span>
      {#if status.fuse_available}
        <span class="badge-ok">Available</span>
      {:else if !status.wsl_available}
        <span class="badge-skip">Requires WSL first</span>
      {:else}
        <span class="badge-warn">Not installed</span>
      {/if}
    </div>
    {#if !status.fuse_available && status.wsl_available}
      <p class="step-desc">fuse3 is needed to mount PBS archives for browsing. Install it in WSL:</p>
      <div class="cmd-block">
        <pre>{CMD_INSTALL_FUSE}</pre>
        <button class="copy-btn" onclick={() => copyText('sudo apt install -y fuse3', 'fuse')}>
          {copied === 'fuse' ? 'Copied!' : 'Copy'}
        </button>
      </div>
      {#if status.fuse_can_install}
        <button class="btn-primary-sm" onclick={doInstallFuse} disabled={installingFuse}>
          {installingFuse ? 'Installing…' : 'Install automatically'}
        </button>
      {/if}
      <p class="step-note">Mounting is optional — restore still works without fuse3.</p>
    {:else if status.fuse_available}
      <p class="step-desc step-ok-text">fuse3 is available in WSL.</p>
    {/if}
  </div>

  <div class="recheck-row">
    <button class="btn-ghost" onclick={load}>Recheck</button>
    {#if status.wsl_available && status.pbs_client_available}
      <span class="all-ok">All required components are installed. You're ready to use BackupPilot.</span>
    {/if}
  </div>
{/if}

<style>
  .page-title { font-size: 20px; font-weight: 600; margin-bottom: 8px; }
  .intro { font-size: 13px; color: var(--text-muted); margin-bottom: 20px; }
  .intro code { font-family: monospace; color: var(--text); }

  .error-box {
    background: #fee2e2; color: #991b1b; border: 1px solid #fca5a5;
    border-radius: var(--radius); padding: 10px 14px; margin-bottom: 14px;
    display: flex; justify-content: space-between; align-items: center; font-size: 13px;
  }
  .close-btn { background: none; border: none; color: #991b1b; font-size: 16px; cursor: pointer; }

  .center { display: flex; justify-content: center; padding: 40px; }

  .na-banner { padding: 16px; font-size: 13px; color: var(--text-muted); }
  .na-banner code { font-family: monospace; color: var(--text); }

  .step { padding: 16px; margin-bottom: 14px; border-left: 4px solid var(--border); }
  .step-ok  { border-left-color: #16a34a; }
  .step-err { border-left-color: #dc2626; }
  .step-disabled { opacity: 0.5; pointer-events: none; }

  .step-header {
    display: flex; align-items: center; gap: 10px; margin-bottom: 10px;
  }
  .step-num {
    width: 24px; height: 24px; border-radius: 50%;
    background: var(--border); color: var(--text);
    display: flex; align-items: center; justify-content: center;
    font-size: 12px; font-weight: 700; flex-shrink: 0;
  }
  .step-title { font-weight: 600; font-size: 14px; flex: 1; }

  .badge-ok   { background: #dcfce7; color: #166534; font-size: 12px; padding: 2px 8px; border-radius: 9999px; }
  .badge-err  { background: #fee2e2; color: #991b1b; font-size: 12px; padding: 2px 8px; border-radius: 9999px; }
  .badge-warn { background: #fef3c7; color: #92400e; font-size: 12px; padding: 2px 8px; border-radius: 9999px; }
  .badge-skip { background: var(--bg); color: var(--text-muted); font-size: 12px; padding: 2px 8px; border-radius: 9999px; border: 1px solid var(--border); }

  .step-desc { font-size: 13px; color: var(--text); margin-bottom: 10px; }
  .step-desc code { font-family: monospace; }
  .step-ok-text { color: var(--text-muted); }
  .step-note { font-size: 12px; color: var(--text-muted); margin-top: 8px; }

  .cmd-block {
    position: relative;
    background: #0d1117; border-radius: var(--radius-sm);
    padding: 12px 48px 12px 14px; margin-bottom: 10px;
    border: 1px solid #30363d;
  }
  .cmd-block pre {
    margin: 0; font-family: monospace; font-size: 12px; color: #c9d1d9;
    white-space: pre-wrap; word-break: break-all;
  }
  .copy-btn {
    position: absolute; top: 8px; right: 8px;
    background: #21262d; border: 1px solid #30363d; color: #c9d1d9;
    font-size: 11px; padding: 3px 8px; border-radius: 4px; cursor: pointer;
  }
  .copy-btn:hover { background: #30363d; }

  .btn-primary-sm {
    background: #2563eb; color: #fff; border: none;
    padding: 6px 14px; border-radius: var(--radius-sm); font-size: 13px;
    cursor: pointer; margin-top: 6px;
  }
  .btn-primary-sm:hover:not(:disabled) { background: #1d4ed8; }
  .btn-primary-sm:disabled { opacity: 0.5; cursor: default; }

  .btn-ghost {
    background: transparent; border: 1px solid var(--border); color: var(--text);
    padding: 8px 18px; border-radius: var(--radius-sm); font-size: 13px; cursor: pointer;
  }
  .btn-ghost:hover { background: var(--bg); }

  .recheck-row { display: flex; align-items: center; gap: 16px; margin-top: 6px; }
  .all-ok { font-size: 13px; color: #16a34a; font-weight: 500; }
</style>
