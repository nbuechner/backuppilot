<script>
  import { onMount, onDestroy } from 'svelte';
  import Profiles        from './pages/Profiles.svelte';
  import Activity        from './pages/Activity.svelte';
  import Restore         from './pages/Restore.svelte';
  import Settings        from './pages/Settings.svelte';
  import EncryptionKeys  from './pages/EncryptionKeys.svelte';
  import Setup           from './pages/Setup.svelte';
  import ProfileForm     from './lib/ProfileForm.svelte';
  import { getSettings, listActiveMounts, unmountSnapshot } from './lib/ipc.js';
  import { applyColorScheme } from './lib/theme.js';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { confirm } from '@tauri-apps/plugin-dialog';

  let page = $state('profiles');
  let advancedMode = $state(false);

  let unlistenClose;

  onMount(async () => {
    try {
      const s = await getSettings();
      applyColorScheme(s.appearance.color_scheme);
      advancedMode = s.appearance.advanced_mode ?? false;
    } catch { /* daemon not ready yet; defaults stay */ }

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
      await appWindow.destroy();
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
</style>
