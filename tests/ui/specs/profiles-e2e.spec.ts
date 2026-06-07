// End-to-end profile lifecycle test:
// Reset all data → re-create profile via GUI → backup → verify activity

const PROFILE = {
    name:        'Windows-Pictures-E2E',
    host:        'pbs-pi.haxxors.com',
    user:        'backuppilot@pbs',
    token:       'win-test=b2c602e8-60f3-4f65-be1c-958cd088cbe4',
    datastore:   'pi-data',
    namespace:   'backuppilot-windows-test',
    backupId:    'DESKTOP-5UIQPLE',
    path:        'C:\\Users\\user\\Pictures\\Test',
};

async function navigateTo(tabLabel: string) {
    const tab = await $(`.nav-item*=${tabLabel}`);
    await tab.waitForClickable({ timeout: 10_000 });
    await tab.click();
    // Activity page IPC can take 60+s on cold daemon start; use 90s for all tabs.
    await browser.waitUntil(
        async () => {
            if (await $('.error-box').isExisting()) return true;
            return !(await $('.spinner').isExisting());
        },
        { timeout: 90_000, timeoutMsg: `${tabLabel} spinner did not clear` }
    );
    await browser.pause(300);
}

describe('Profile lifecycle', () => {
    before(async () => {
        await browser.url('http://tauri.localhost/');
        // Force a full page reload to reset Svelte state left by the previous spec.
        // Same-URL navigation in WebView2 may be a no-op, leaving showProfileForm=true.
        await browser.execute(() => window.location.reload());
        await browser.waitUntil(
            async () => {
                const html = await browser.execute(() => document.body.innerHTML) as string;
                return html.length > 200;
            },
            { timeout: 30_000, timeoutMsg: 'App body remained empty' }
        );
    });

    // ── 1. Reset all data ──────────────────────────────────────────────────────

    it('Reset All Data clears all profiles and data', async () => {
        await navigateTo('Settings');

        const resetBtn = await $('button*=Reset All Data');
        await resetBtn.waitForClickable({ timeout: 5_000 });
        await resetBtn.click();

        const confirmBtn = await $('button*=Yes, Reset Everything');
        await confirmBtn.waitForDisplayed({ timeout: 5_000 });
        await confirmBtn.click();

        // App shows an in-page banner after reset (no native alert)
        const banner = await $('.reset-done-banner');
        await banner.waitForDisplayed({ timeout: 10_000 });
    });

    it('Profiles tab is empty after reset', async () => {
        await navigateTo('Profiles');
        const cards = await $$('.profile-card');
        let count = 0;
        for (const _ of cards) count++;
        expect(count).toBe(0);
    });

    // ── 2. Create profile via GUI ──────────────────────────────────────────────

    it('creates a new profile via the Add Profile form', async () => {
        const addBtn = await $('button*=Add Profile');
        await addBtn.waitForClickable({ timeout: 5_000 });
        await addBtn.click();

        // Wait for form dialog
        await $('#pf-name').waitForDisplayed({ timeout: 5_000 });

        await $('#pf-name').setValue(PROFILE.name);
        await $('#pf-host').setValue(PROFILE.host);
        await $('#pf-user').setValue(PROFILE.user);
        await $('#pf-token').setValue(PROFILE.token);
        await $('#pf-ds').setValue(PROFILE.datastore);
        await $('#pf-ns').setValue(PROFILE.namespace);
        await $('#pf-bid').setValue(PROFILE.backupId);

        // Add backup path
        const pathInput = await $('input[placeholder*="Users"]');
        await pathInput.setValue(PROFILE.path);
        const addPathBtn = await $('button*=Add');
        await addPathBtn.click();
        await browser.pause(200);

        // Save
        const saveBtn = await $('.dialog-footer .btn-primary');
        await saveBtn.waitForClickable({ timeout: 3_000 });
        await saveBtn.click();

        // Form should close
        await browser.waitUntil(
            async () => !(await $('#pf-name').isExisting()),
            { timeout: 10_000, timeoutMsg: 'Profile form did not close after save' }
        );
    });

    it('new profile card appears in the list', async () => {
        const card = await $(`.profile-card`);
        await card.waitForDisplayed({ timeout: 10_000 });
        const nameEl = await card.$('.profile-name');
        const name = await nameEl.getText();
        expect(name).toBe(PROFILE.name);
    });

    // ── 3. Trigger backup ─────────────────────────────────────────────────────

    it('Backup Now triggers a backup run', async () => {
        const backupBtn = await $('button*=Backup Now');
        await backupBtn.waitForClickable({ timeout: 5_000 });
        await backupBtn.click();

        // Button should briefly show "Starting…" then revert — wait for revert.
        // Preflight includes a cold-WSL probe that can take up to ~30s on first run.
        await browser.waitUntil(
            async () => {
                const text = await backupBtn.getText();
                return text.includes('Backup Now');
            },
            { timeout: 60_000, timeoutMsg: 'Backup Now button did not revert after triggering' }
        );
    });

    it('Activity tab shows a successful backup run', async () => {
        await navigateTo('Activity');

        // Wait for a SUCCESS or RUNNING row (backup may still be in progress)
        await browser.waitUntil(
            async () => {
                for (const row of await $$('.log-row')) {
                    const text = (await row.getText()).toLowerCase();
                    if (text.includes(PROFILE.name.toLowerCase())) return true;
                }
                return false;
            },
            { timeout: 120_000, timeoutMsg: 'No activity row appeared for the backup' }
        );

        // Wait for the run to finish (any terminal state — not RUNNING)
        await browser.waitUntil(
            async () => {
                for (const row of await $$('.log-row')) {
                    const text = (await row.getText()).toLowerCase();
                    if (!text.includes(PROFILE.name.toLowerCase())) continue;
                    return text.includes('success') || text.includes('failed') || text.includes('skipped');
                }
                return false;
            },
            { timeout: 180_000, timeoutMsg: 'Backup run did not complete within 3 minutes' }
        );

        // Verify it succeeded (not failed or skipped)
        let status = '';
        for (const row of await $$('.log-row')) {
            const text = (await row.getText()).toLowerCase();
            if (!text.includes(PROFILE.name.toLowerCase())) continue;
            if (text.includes('success')) { status = 'success'; break; }
            if (text.includes('failed'))  { status = 'failed';  break; }
            if (text.includes('skipped')) { status = 'skipped'; break; }
        }
        expect(status).toBe('success');
    });
});
