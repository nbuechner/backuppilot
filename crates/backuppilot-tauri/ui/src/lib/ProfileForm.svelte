<script>
  import { ipcCall } from './ipc.js';

  let { onSaved, onCancel } = $props();

  // Basic
  let name       = $state('');
  let enabled    = $state(true);

  // PBS connection
  let pbsHost      = $state('');
  let pbsUser      = $state('root@pam');
  let pbsToken     = $state('');
  let pbsDatastore = $state('');
  let pbsFingerprint = $state('');

  // Backup config
  let backupId   = $state('');
  let namespace  = $state('');
  let pathInput  = $state('');
  let paths      = $state([]);

  // Schedule
  let scheduleType = $state('daily');
  let scheduleTime = $state('02:00');
  let scheduleWeekday = $state(1);

  let saving = $state(false);
  let error  = $state('');

  // Populate backup ID from hostname
  async function loadHostname() {
    try {
      const v = await ipcCall('version', null);
      // Hostname not returned by version — use env fallback below
    } catch {}
    // Browser-side: navigator doesn't give hostname; leave blank for user to fill
  }
  loadHostname();

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

    const newProfile = {
      name: name.trim(),
      enabled,
      repository,
      namespace: namespace.trim() || null,
      backup_id: backupId.trim() || 'localhost',
      paths,
      excludes: [],
      schedule: buildSchedule(),
      conditions: {
        execution_context: 'daemon',
        require_ac_power: false,
        require_network: [],
        require_vpn: false,
        require_server_reachable: true,
      },
      health_check: {},
    };

    saving = true;
    try {
      await ipcCall('create_profile', { profile_json: JSON.stringify(newProfile) });
      onSaved();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="overlay" onclick={onCancel}>
  <div class="dialog" onclick={e => e.stopPropagation()}>
    <div class="dialog-header">
      <h2>New Backup Profile</h2>
      <button class="close-btn" onclick={onCancel}>✕</button>
    </div>

    <div class="dialog-body">

      <!-- Basic -->
      <section>
        <h3 class="section-title">General</h3>
        <div class="field">
          <label>Profile name</label>
          <input type="text" bind:value={name} placeholder="My Backup" />
        </div>
        <div class="field row">
          <label>Enabled</label>
          <input type="checkbox" bind:checked={enabled} />
        </div>
      </section>

      <!-- PBS Connection -->
      <section>
        <h3 class="section-title">PBS Connection</h3>
        <div class="field">
          <label>Host</label>
          <input type="text" bind:value={pbsHost} placeholder="192.168.1.100 or pbs.example.com" />
        </div>
        <div class="field-row">
          <div class="field">
            <label>User</label>
            <input type="text" bind:value={pbsUser} placeholder="root@pam" />
          </div>
          <div class="field">
            <label>Datastore</label>
            <input type="text" bind:value={pbsDatastore} placeholder="backups" />
          </div>
        </div>
        <div class="field">
          <label>API Token</label>
          <input type="password" bind:value={pbsToken} placeholder="tokenid=secret" />
        </div>
        <div class="field">
          <label>TLS Fingerprint <span class="optional">(optional)</span></label>
          <input type="text" bind:value={pbsFingerprint} placeholder="XX:XX:XX:..." />
        </div>
      </section>

      <!-- Backup -->
      <section>
        <h3 class="section-title">Backup</h3>
        <div class="field">
          <label>Backup ID <span class="optional">(hostname)</span></label>
          <input type="text" bind:value={backupId} placeholder="my-pc" />
        </div>
        <div class="field">
          <label>Namespace <span class="optional">(optional)</span></label>
          <input type="text" bind:value={namespace} placeholder="team/project" />
        </div>
        <div class="field">
          <label>Backup paths</label>
          <div class="path-list">
            {#each paths as p}
              <div class="path-item">
                <span>{p}</span>
                <button class="remove-btn" onclick={() => removePath(p)}>✕</button>
              </div>
            {/each}
            <div class="path-add">
              <input
                type="text"
                bind:value={pathInput}
                placeholder="C:\Users\user\Documents"
                onkeydown={e => e.key === 'Enter' && addPath()}
              />
              <button class="btn-ghost add-btn" onclick={addPath}>Add</button>
            </div>
          </div>
        </div>
      </section>

      <!-- Schedule -->
      <section>
        <h3 class="section-title">Schedule</h3>
        <div class="field-row">
          <div class="field">
            <label>Type</label>
            <select bind:value={scheduleType}>
              <option value="manual">Manual only</option>
              <option value="daily">Daily</option>
              <option value="weekly">Weekly</option>
            </select>
          </div>
          {#if scheduleType !== 'manual'}
            <div class="field">
              <label>Time</label>
              <input type="time" bind:value={scheduleTime} />
            </div>
          {/if}
          {#if scheduleType === 'weekly'}
            <div class="field">
              <label>Weekday</label>
              <select bind:value={scheduleWeekday}>
                <option value={1}>Monday</option>
                <option value={2}>Tuesday</option>
                <option value={3}>Wednesday</option>
                <option value={4}>Thursday</option>
                <option value={5}>Friday</option>
                <option value={6}>Saturday</option>
                <option value={7}>Sunday</option>
              </select>
            </div>
          {/if}
        </div>
      </section>

      {#if error}
        <div class="error-box">{error}</div>
      {/if}

    </div>

    <div class="dialog-footer">
      <button class="btn-ghost" onclick={onCancel}>Cancel</button>
      <button class="btn-primary" disabled={saving} onclick={save}>
        {saving ? 'Saving…' : 'Create Profile'}
      </button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.45);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .dialog {
    background: #fff;
    border-radius: 10px;
    width: 620px;
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 8px 40px rgba(0,0,0,0.25);
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 18px 24px 14px;
    border-bottom: 1px solid var(--border);
  }

  .dialog-header h2 { font-size: 16px; font-weight: 600; }

  .close-btn {
    background: transparent;
    border: none;
    font-size: 16px;
    color: var(--text-muted);
    padding: 4px 8px;
    cursor: pointer;
  }
  .close-btn:hover { color: var(--text); }

  .dialog-body {
    overflow-y: auto;
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 14px 24px;
    border-top: 1px solid var(--border);
  }

  section { display: flex; flex-direction: column; gap: 10px; }

  .section-title {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
    margin-bottom: 2px;
  }

  .field { display: flex; flex-direction: column; gap: 4px; }
  .field.row { flex-direction: row; align-items: center; gap: 10px; }

  .field-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }

  label { font-size: 12px; font-weight: 500; color: var(--text); }

  .optional { font-weight: 400; color: var(--text-muted); }

  input[type="text"],
  input[type="password"],
  input[type="time"],
  select {
    padding: 7px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 13px;
    background: #fff;
    color: var(--text);
    outline: none;
    width: 100%;
  }
  input:focus, select:focus { border-color: var(--blue); }

  .path-list { display: flex; flex-direction: column; gap: 6px; }

  .path-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 6px 10px;
    font-size: 13px;
  }

  .remove-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 12px;
    padding: 0 4px;
    cursor: pointer;
  }
  .remove-btn:hover { color: var(--red); }

  .path-add {
    display: flex;
    gap: 8px;
  }
  .path-add input { flex: 1; }
  .add-btn { white-space: nowrap; }

  .error-box {
    background: #f8d7da;
    color: #721c24;
    border: 1px solid #f5c6cb;
    border-radius: var(--radius-sm);
    padding: 10px 14px;
    font-size: 13px;
  }
</style>
