<script>
  import { ipcCall, verifyCredentials } from './ipc.js';

  let { onSaved, onCancel, profile = null, advancedMode = false } = $props();

  const isEdit = profile != null;

  // Parse existing repository JSON if editing
  function parseRepo(repoJson) {
    try { return JSON.parse(repoJson); } catch { return {}; }
  }
  const existingRepo = isEdit ? parseRepo(profile.repository) : {};

  // Basic
  let name       = $state(isEdit ? profile.name : '');
  let enabled    = $state(isEdit ? profile.enabled : true);

  // PBS connection
  let pbsHost        = $state(existingRepo.host ?? '');
  let pbsUser        = $state(existingRepo.user ?? 'root@pam');
  let pbsToken       = $state(existingRepo.token ?? '');
  let pbsDatastore   = $state(existingRepo.datastore ?? '');
  let pbsFingerprint = $state(profile?.server_fingerprint ?? '');

  // Backup config
  let backupId  = $state(isEdit ? (profile.backup_id ?? '') : '');
  let namespace = $state(isEdit ? (profile.namespace ?? '') : '');
  let pathInput = $state('');
  let paths     = $state(isEdit ? [...(profile.paths ?? [])] : []);

  // Schedule
  function parseSchedule(s) {
    if (!s) return { type: 'daily', time: '02:00', weekday: 1 };
    return { type: s.type ?? 'daily', time: s.time ?? '02:00', weekday: s.weekday ?? 1 };
  }
  const sched = parseSchedule(profile?.schedule);
  let scheduleType    = $state(sched.type);
  let scheduleTime    = $state(sched.time);
  let scheduleWeekday = $state(sched.weekday);

  let saving     = $state(false);
  let error      = $state('');
  let testing    = $state(false);
  let testResult = $state(null); // null | { ok: boolean, message?: string }

  // Clear stale test result when any PBS connection field changes.
  $effect(() => {
    void (pbsHost, pbsUser, pbsToken, pbsDatastore, pbsFingerprint, namespace);
    testResult = null;
  });

  async function testConnection() {
    testing = true;
    testResult = null;
    try {
      const result = await verifyCredentials({
        repository: JSON.stringify({
          user: pbsUser.trim(),
          host: pbsHost.trim(),
          token: pbsToken.trim(),
          datastore: pbsDatastore.trim(),
        }),
        namespace: namespace.trim() || null,
        server_fingerprint: pbsFingerprint.trim() || null,
      });
      testResult = result;
    } catch (e) {
      testResult = { ok: false, message: String(e) };
    } finally {
      testing = false;
    }
  }

  function onKeydown(e) {
    if (e.key === 'Escape') onCancel();
  }

  function addPath() {
    const p = pathInput.trim();
    if (p && !paths.includes(p)) paths = [...paths, p];
    pathInput = '';
  }

  function removePath(p) {
    paths = paths.filter(x => x !== p);
  }

  function buildSchedule() {
    const s = { type: scheduleType };
    if (scheduleType === 'daily')  s.time = scheduleTime;
    if (scheduleType === 'weekly') { s.time = scheduleTime; s.weekday = scheduleWeekday; }
    return s;
  }

  async function save() {
    error = '';
    if (!name.trim())        { error = 'Profile name is required.'; return; }
    if (!pbsHost.trim())     { error = 'PBS host is required.'; return; }
    if (!pbsDatastore.trim()){ error = 'Datastore is required.'; return; }
    if (!pbsToken.trim())    { error = 'API token is required.'; return; }
    if (paths.length === 0)  { error = 'Add at least one backup path.'; return; }

    const repository = JSON.stringify({
      user: pbsUser.trim(),
      host: pbsHost.trim(),
      token: pbsToken.trim(),
      datastore: pbsDatastore.trim(),
    });

    const profileData = {
      name: name.trim(),
      enabled,
      repository,
      namespace: namespace.trim() || null,
      backup_id: backupId.trim() || 'localhost',
      paths,
      excludes: profile?.excludes ?? [],
      schedule: buildSchedule(),
      conditions: profile?.conditions ?? {
        execution_context: 'desktop',
        require_ac_power: false,
        require_network: [],
        require_vpn: false,
        require_server_reachable: true,
      },
      health_check: profile?.health_check ?? {},
    };
    if (pbsFingerprint.trim()) profileData.server_fingerprint = pbsFingerprint.trim();

    saving = true;
    try {
      if (isEdit) {
        await ipcCall('update_profile', {
          profile_id: profile.id,
          profile_json: JSON.stringify(profileData),
        });
      } else {
        await ipcCall('create_profile', { profile_json: JSON.stringify(profileData) });
      }
      onSaved();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="overlay" onclick={onCancel}>
  <div class="dialog" onclick={e => e.stopPropagation()}>
    <div class="dialog-header">
      <h2>{isEdit ? 'Edit Profile' : 'New Backup Profile'}</h2>
      <button class="close-btn" onclick={onCancel}>✕</button>
    </div>

    <div class="dialog-body">

      <!-- General -->
      <section>
        <h3 class="section-title">General</h3>
        <div class="field">
          <label for="pf-name">Profile name</label>
          <input id="pf-name" type="text" placeholder="My Backup" bind:value={name} />
        </div>
        <div class="field">
          <label for="pf-enabled">Enabled</label>
          <input id="pf-enabled" type="checkbox" bind:checked={enabled} />
        </div>
      </section>

      <!-- PBS Connection -->
      <section>
        <h3 class="section-title">PBS Connection</h3>
        <div class="field">
          <label for="pf-host">PBS host</label>
          <input id="pf-host" type="text" placeholder="192.168.1.100 or pbs.example.com" bind:value={pbsHost} />
        </div>
        <div class="field">
          <label for="pf-user">User</label>
          <input id="pf-user" type="text" placeholder="root@pam" bind:value={pbsUser} />
        </div>
        <div class="field">
          <label for="pf-token">API token</label>
          <input id="pf-token" type="text" placeholder="tokenid=secret" bind:value={pbsToken} />
        </div>
        <div class="field">
          <label for="pf-ds">Datastore</label>
          <input id="pf-ds" type="text" placeholder="backups" bind:value={pbsDatastore} />
        </div>
        <div class="field">
          <label for="pf-fp">Fingerprint</label>
          <input id="pf-fp" type="text" placeholder="AA:BB:CC:… (optional)" bind:value={pbsFingerprint} />
        </div>
        <div class="field">
          <span></span>
          <div class="test-row">
            <button class="btn-ghost" disabled={testing} onclick={testConnection}>
              {testing ? 'Testing…' : 'Test Connection'}
            </button>
            {#if testResult}
              <span class="test-badge {testResult.ok ? 'test-ok' : 'test-fail'}">
                {testResult.ok ? '✓ Connected' : '✗ ' + (testResult.message ?? 'Failed')}
              </span>
            {/if}
          </div>
        </div>
      </section>

      <!-- Backup -->
      <section>
        <h3 class="section-title">Backup</h3>
        <div class="field">
          <label for="pf-bid">Backup ID</label>
          <input id="pf-bid" type="text" placeholder="hostname" bind:value={backupId} />
        </div>
        <div class="field">
          <label for="pf-ns">Namespace</label>
          <input id="pf-ns" type="text" placeholder="optional" bind:value={namespace} />
        </div>
        <div class="field">
          <label>Backup paths</label>
          <div class="path-input-row">
            <input type="text" placeholder="C:\Users\user\Documents" bind:value={pathInput}
              onkeydown={e => e.key === 'Enter' && addPath()} />
            <button class="btn-ghost-sm" onclick={addPath}>Add</button>
          </div>
        </div>
        {#if paths.length > 0}
          <div class="path-list">
            {#each paths as p}
              <div class="path-tag">
                <span class="mono">{p}</span>
                <button onclick={() => removePath(p)}>×</button>
              </div>
            {/each}
          </div>
        {/if}
      </section>

      <!-- Schedule — advanced mode only -->
      {#if advancedMode}
      <section>
        <h3 class="section-title">Schedule</h3>
        <div class="field">
          <label for="pf-sched">Type</label>
          <select id="pf-sched" bind:value={scheduleType}>
            <option value="manual">Manual</option>
            <option value="daily">Daily</option>
            <option value="weekly">Weekly</option>
          </select>
        </div>
        {#if scheduleType !== 'manual'}
          <div class="field">
            <label for="pf-time">Time</label>
            <input id="pf-time" type="time" bind:value={scheduleTime} />
          </div>
        {/if}
        {#if scheduleType === 'weekly'}
          <div class="field">
            <label for="pf-wday">Weekday</label>
            <select id="pf-wday" bind:value={scheduleWeekday}>
              <option value={1}>Monday</option>
              <option value={2}>Tuesday</option>
              <option value={3}>Wednesday</option>
              <option value={4}>Thursday</option>
              <option value={5}>Friday</option>
              <option value={6}>Saturday</option>
              <option value={0}>Sunday</option>
            </select>
          </div>
        {/if}
      </section>
      {/if}

    </div>

    {#if error}
      <div class="error-box">{error}</div>
    {/if}

    <div class="dialog-footer">
      <button class="btn-primary" onclick={save} disabled={saving}>
        {saving ? 'Saving…' : (isEdit ? 'Save Changes' : 'Create Profile')}
      </button>
      <button class="btn-ghost" onclick={onCancel}>Cancel</button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.72);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    z-index: 50;
    padding: 40px 20px;
    overflow-y: auto;
  }

  .dialog {
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: 0 20px 60px rgba(0,0,0,0.25);
    width: 100%;
    max-width: 540px;
    display: flex;
    flex-direction: column;
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px 16px;
    border-bottom: 1px solid var(--border);
  }
  .dialog-header h2 { font-size: 16px; font-weight: 700; margin: 0; }

  .close-btn {
    background: none; border: none; font-size: 18px;
    color: var(--text-muted); cursor: pointer; padding: 2px 6px;
  }
  .close-btn:hover { color: var(--text); }

  .dialog-body { padding: 20px 24px; display: flex; flex-direction: column; gap: 20px; overflow-y: auto; max-height: 60vh; }

  section { display: flex; flex-direction: column; gap: 10px; }

  .section-title {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    margin: 0;
  }

  .field {
    display: grid;
    grid-template-columns: 130px 1fr;
    align-items: center;
    gap: 10px;
  }
  .field label { font-size: 13px; color: var(--text); }
  .field input[type="text"],
  .field input[type="time"],
  .field select {
    padding: 7px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 13px;
    background: var(--bg);
    color: var(--text);
    width: 100%;
  }
  .field input[type="checkbox"] { width: 16px; height: 16px; }

  .path-input-row { display: flex; gap: 6px; }
  .path-input-row input { flex: 1; padding: 7px 10px; border: 1px solid var(--border); border-radius: var(--radius-sm); font-size: 13px; background: var(--bg); }

  .path-list { display: flex; flex-direction: column; gap: 4px; padding-left: 140px; }
  .path-tag {
    display: flex; align-items: center; justify-content: space-between;
    background: var(--bg); border: 1px solid var(--border);
    border-radius: var(--radius-sm); padding: 4px 8px; font-size: 12px;
  }
  .path-tag button {
    background: none; border: none; color: var(--text-muted); cursor: pointer; font-size: 14px;
  }
  .mono { font-family: monospace; }

  .test-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .test-badge {
    font-size: 12px;
    font-weight: 500;
  }
  .test-ok   { color: var(--green); }
  .test-fail { color: var(--red); }

  .error-box {
    margin: 0 24px 16px;
    background: #f8d7da; color: #721c24;
    border: 1px solid #f5c6cb; border-radius: var(--radius-sm); padding: 10px 14px; font-size: 13px;
  }

  .dialog-footer {
    display: flex;
    gap: 10px;
    padding: 16px 24px;
    border-top: 1px solid var(--border);
  }

  .btn-ghost-sm {
    background: transparent; border: 1px solid var(--border);
    padding: 5px 12px; border-radius: var(--radius-sm); font-size: 13px;
    cursor: pointer; color: var(--text); white-space: nowrap;
  }
  .btn-ghost {
    background: transparent; border: 1px solid var(--border);
    padding: 8px 18px; border-radius: var(--radius-sm); font-size: 13px;
    font-weight: 500; cursor: pointer; color: var(--text);
  }
  .btn-ghost:hover { background: var(--bg); }
</style>
