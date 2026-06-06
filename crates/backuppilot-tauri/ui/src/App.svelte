<script>
  import Profiles        from './pages/Profiles.svelte';
  import Activity        from './pages/Activity.svelte';
  import Restore         from './pages/Restore.svelte';
  import Settings        from './pages/Settings.svelte';
  import EncryptionKeys  from './pages/EncryptionKeys.svelte';

  let page = $state('profiles');

  const nav = [
    { id: 'profiles', label: 'Profiles',        icon: '🗂' },
    { id: 'activity', label: 'Activity',         icon: '📋' },
    { id: 'restore',  label: 'Restore',          icon: '⏮' },
    { id: 'keys',     label: 'Encryption Keys',  icon: '🔑' },
    { id: 'settings', label: 'Settings',         icon: '⚙' },
  ];
</script>

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
      <Profiles />
    {:else if page === 'activity'}
      <Activity />
    {:else if page === 'restore'}
      <Restore />
    {:else if page === 'keys'}
      <EncryptionKeys />
    {:else if page === 'settings'}
      <Settings />
    {/if}
  </main>
</div>

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
