<script>
  import { onMount, onDestroy } from 'svelte';
  import Profiles        from './pages/Profiles.svelte';
  import Activity        from './pages/Activity.svelte';
  import Restore         from './pages/Restore.svelte';
  import Settings        from './pages/Settings.svelte';
  import EncryptionKeys  from './pages/EncryptionKeys.svelte';
  import Setup           from './pages/Setup.svelte';
  import ProfileForm     from './lib/ProfileForm.svelte';
  import { getSettings, listActiveMounts, unmountSnapshot, checkWindowsSetup, checkPbsClientAvailable } from './lib/ipc.js';
  import { applyColorScheme } from './lib/theme.js';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { confirm } from '@tauri-apps/plugin-dialog';

  let page = $state('profiles');
  let advancedMode = $state(false);
  let pbsAvailable = $state(true);
  let isWindows = $state(false);
  let pbsBannerDismissed = $state(false);

  const CMD_INSTALL_PBS_LINUX = `# Add Proxmox repository (Debian/Ubuntu)
curl -fsSL https://enterprise.proxmox.com/debian/proxmox-release-bookworm.gpg \\
  | sudo tee /etc/apt/trusted.gpg.d/proxmox-release-bookworm.gpg >/dev/null
echo "deb http://download.proxmox.com/debian/pbs-client bookworm main" \\
  | sudo tee /etc/apt/sources.list.d/pbs-client.list
sudo apt-get update && sudo apt-get install -y proxmox-backup-client`;

  let unlistenClose;
  let pbsCopied = $state(false);

  function copyPbsCmd() {
    navigator.clipboard.writeText(CMD_INSTALL_PBS_LINUX).then(() => {
      pbsCopied = true;
      setTimeout(() => { pbsCopied = false; }, 2000);
    });
  }

  async function checkPbs() {
    try {
      const setup = await checkWindowsSetup();
      isWindows = setup.applicable;
      if (isWindows) {
        pbsAvailable = setup.pbs_client_available;
      } else {
        pbsAvailable = await checkPbsClientAvailable();
      }
    } catch { /* daemon not ready; keep optimistic default */ }
  }

  onMount(async () => {
    try {
      const s = await getSettings();
      applyColorScheme(s.appearance.color_scheme);
      advancedMode = s.appearance.advanced_mode ?? false;
    } catch { /* daemon not ready yet; defaults stay */ }

    checkPbs();

    const appWindow = getCurrentWindow();
    unlistenClose = await appWindow.onCloseRequested(async (event) => {
      event.preventDefault();
      try {
        const s = await getSettings();
        if (s.tray.ask_unmount_on_quit) {
          const mounts = await listActiveMounts();
          if (mounts.length > 0) {
            const unmount = await confirm(
              `You have ${mounts.length} active mount(s). Unmount before closing?`,
              { title: 'Active Mounts', okLabel: 'Unmount & Close', cancelLabel: 'Cancel' }
            );
            if (!unmount) return;
            for (const m of mounts) {
              await unmountSnapshot(m.id).catch(() => {});
            }
          }
        }
      } catch { /* fall through and close */ }
      await appWindow.destroy().catch(() => appWindow.close());
    });
  });

  onDestroy(() => { if (unlistenClose) unlistenClose(); });

  // Profile form state lives here so ProfileForm's position:fixed overlay
  // renders as a sibling of .layout, not inside .content (overflow-y:auto),
  // which would clip it in WebView2.
  let showProfileForm  = $state(false);
  let editFormProfile  = $state(null);
  // Incrementing this after a save tells Profiles to reload its list immediately.
  let profileReloadKey = $state(0);

  function openProfileForm(profile) {
    editFormProfile = profile;
    showProfileForm = true;
  }

  function closeProfileForm() {
    showProfileForm = false;
    editFormProfile = null;
    profileReloadKey++;
  }

  function onKeydown(e) {
    if ((e.ctrlKey || e.metaKey) && e.key === 'n' && page === 'profiles' && !showProfileForm) {
      e.preventDefault();
      openProfileForm(null);
    }
  }

  const nav = [
    { id: 'profiles', label: 'Profiles',        icon: '🗂' },
    { id: 'activity', label: 'Activity',         icon: '📋' },
    { id: 'restore',  label: 'Restore',          icon: '⏮' },
    { id: 'keys',     label: 'Encryption Keys',  icon: '🔑' },
    { id: 'settings', label: 'Settings',         icon: '⚙' },
    { id: 'setup',    label: 'Setup Guide',      icon: '🛠' },
  ];
</script>

<svelte:window onkeydown={onKeydown} />

<div class="layout">
  <nav class="sidebar">
    <div class="sidebar-header">
      <span class="app-name">BackupPilot</span>
    </div>
    {#each nav as item}
      <button
        class="nav-item"
        class:active={page === item.id}
        onclick={() => page = item.id}
      >
        <span class="nav-icon">{item.icon}</span>
        {item.label}
      </button>
    {/each}
  </nav>

  <main class="content">
    {#if !pbsAvailable && !pbsBannerDismissed}
      <div class="pbs-banner">
        <div class="pbs-banner-body">
          <strong>proxmox-backup-client is not installed.</strong>
          {#if isWindows}
            BackupPilot requires it inside WSL to run backups and restores.
            <button class="pbs-link-btn" onclick={() => page = 'setup'}>Open Setup Guide</button>
          {:else}
            Install it to enable backups and restores:
            <div class="pbs-cmd-block">
              <pre>{CMD_INSTALL_PBS_LINUX}</pre>
              <button class="pbs-copy-btn" onclick={copyPbsCmd}>{pbsCopied ? 'Copied!' : 'Copy'}</button>
            </div>
          {/if}
        </div>
        <button class="pbs-dismiss-btn" onclick={() => pbsBannerDismissed = true} title="Dismiss">x</button>
      </div>
    {/if}

    {#if page === 'profiles'}
      <Profiles onOpenForm={openProfileForm} reloadKey={profileReloadKey} />
    {:else if page === 'activity'}
      <Activity />
    {:else if page === 'restore'}
      <Restore />
    {:else if page === 'keys'}
      <EncryptionKeys />
    {:else if page === 'settings'}
      <Settings />
    {:else if page === 'setup'}
      <Setup />
    {/if}
  </main>
</div>

{#if showProfileForm}
  <ProfileForm
    profile={editFormProfile}
    {advancedMode}
    onSaved={closeProfileForm}
    onCancel={closeProfileForm}
  />
{/if}

<style>
  .layout {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }

  .sidebar {
    width: var(--sidebar-width);
    background: var(--sidebar-bg);
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    padding: 8px 0;
  }

  .sidebar-header {
    padding: 12px 16px 16px;
    border-bottom: 1px solid #2e3d52;
    margin-bottom: 8px;
  }

  .app-name {
    color: #fff;
    font-weight: 700;
    font-size: 15px;
    letter-spacing: -0.01em;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 9px 16px;
    background: transparent;
    color: var(--sidebar-text);
    border-radius: 0;
    font-size: 13.5px;
    font-weight: 500;
    text-align: left;
    border: none;
  }

  .nav-item:hover { background: var(--sidebar-hover); opacity: 1; }

  .nav-item.active {
    background: var(--sidebar-active);
    color: var(--sidebar-text-active);
  }

  .nav-icon { font-size: 15px; }

  .content {
    flex: 1;
    overflow-y: auto;
    background: var(--bg);
    padding: 24px;
  }

  .pbs-banner {
    background: #fef3c7;
    border: 1px solid #fbbf24;
    border-radius: var(--radius, 6px);
    padding: 12px 14px;
    margin-bottom: 18px;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    font-size: 13px;
    color: #78350f;
  }

  .pbs-banner-body { flex: 1; }
  .pbs-banner-body strong { display: block; margin-bottom: 4px; font-weight: 600; }

  .pbs-link-btn {
    background: none; border: none; color: #92400e; text-decoration: underline;
    cursor: pointer; font-size: 13px; padding: 0; margin-left: 4px;
  }
  .pbs-link-btn:hover { color: #78350f; }

  .pbs-cmd-block {
    position: relative;
    background: #0d1117;
    border-radius: 4px;
    padding: 10px 48px 10px 12px;
    margin-top: 8px;
    border: 1px solid #30363d;
  }
  .pbs-cmd-block pre {
    margin: 0; font-family: monospace; font-size: 11px;
    color: #c9d1d9; white-space: pre-wrap; word-break: break-all;
  }
  .pbs-copy-btn {
    position: absolute; top: 6px; right: 6px;
    background: #21262d; border: 1px solid #30363d; color: #c9d1d9;
    font-size: 11px; padding: 2px 7px; border-radius: 3px; cursor: pointer;
  }
  .pbs-copy-btn:hover { background: #30363d; }

  .pbs-dismiss-btn {
    background: none; border: none; color: #92400e; font-size: 16px;
    cursor: pointer; line-height: 1; flex-shrink: 0; padding: 0 2px;
  }
  .pbs-dismiss-btn:hover { color: #78350f; }
</style>
