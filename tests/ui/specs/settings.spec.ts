// Settings page UI tests via WebdriverIO + tauri-driver

describe('Settings page', () => {
    before(async () => {
        await browser.url('http://tauri.localhost/');
        await browser.waitUntil(
            async () => {
                const html = await browser.execute(() => document.body.innerHTML) as string;
                return html.length > 200;
            },
            { timeout: 30_000, timeoutMsg: 'App body remained empty after navigation' }
        );
        const settingsTab = await $('.nav-item*=Settings');
        await settingsTab.waitForClickable({ timeout: 10_000 });
        await settingsTab.click();
        await browser.waitUntil(
            async () => !(await $('.spinner').isExisting()),
            { timeout: 15_000, timeoutMsg: 'Settings page spinner did not clear' }
        );
        await browser.pause(300);
    });

    it('loads without error and shows settings form', async () => {
        const errorBox = await $('.error-box');
        if (await errorBox.isExisting()) {
            const msg = await errorBox.getText();
            throw new Error(`Settings page shows error: ${msg}`);
        }
        // Settings form should be visible
        const sections = await $$('.section');
        expect(sections.length).toBeGreaterThan(0);
    });

    it('shows all expected sections', async () => {
        const headers = await $$('.section-header');
        const texts = await Promise.all(headers.map(h => h.getText()));
        const joined = texts.join(' ');
        for (const section of ['Notifications', 'Appearance', 'Health Check', 'Backup Conditions', 'Log Retention', 'Updates']) {
            expect(joined).toContain(section);
        }
    });

    it('notifications toggle shows and hides sub-settings', async () => {
        // Find the "Enable notifications" toggle row
        const toggleRows = await $$('.toggle-row');
        let notifToggle: WebdriverIO.Element | null = null;
        for (const row of toggleRows) {
            const t = await row.getText();
            if (t.includes('Enable notifications')) {
                notifToggle = await row.$('input[type="checkbox"]');
                break;
            }
        }
        if (!notifToggle) throw new Error('"Enable notifications" toggle not found');

        const wasChecked = await notifToggle.isSelected();

        // Make sure notifications are enabled to see sub-settings
        if (!wasChecked) {
            await notifToggle.click();
            await browser.pause(500);
        }

        const subSettings = await $('.sub-settings');
        expect(await subSettings.isDisplayed()).toBe(true);

        // Disable and confirm sub-settings collapse
        await notifToggle.click();
        await browser.pause(500);
        expect(await subSettings.isExisting()).toBe(false);

        // Restore original state
        if (wasChecked) {
            await notifToggle.click();
            await browser.pause(500);
        }
    });

    it('color scheme select has valid options', async () => {
        await $('#color-scheme').waitForExist({ timeout: 3_000 });
        const options = await $$('#color-scheme option');
        const values = await Promise.all(options.map(o => o.getValue()));
        expect(values).toContain('system');
        expect(values).toContain('light');
        expect(values).toContain('dark');
    });

    it('health check inputs contain positive numbers', async () => {
        const warnInput    = await $('#warn-days');
        const criticalInput = await $('#critical-days');
        await warnInput.waitForExist({ timeout: 3_000 });
        const warnVal     = parseInt(await warnInput.getValue(), 10);
        const criticalVal = parseInt(await criticalInput.getValue(), 10);
        expect(warnVal).toBeGreaterThan(0);
        expect(criticalVal).toBeGreaterThan(0);
    });

    it('Save Settings button works and shows saved badge', async () => {
        const saveBtn = await $('button*=Save Settings');
        await saveBtn.waitForClickable({ timeout: 3_000 });
        await saveBtn.click();

        // Saved badge should appear within 3 seconds
        const savedBadge = await $('.saved-badge');
        await savedBadge.waitForDisplayed({ timeout: 3_000 });
        expect(await savedBadge.isDisplayed()).toBe(true);
    });

    it('Reset All Data shows confirmation and Cancel dismisses it', async () => {
        const resetBtn = await $('button*=Reset All Data');
        await resetBtn.waitForClickable({ timeout: 3_000 });
        await resetBtn.click();

        // Confirm dialog should appear
        const confirmBtn = await $('button*=Yes, Reset Everything');
        await confirmBtn.waitForDisplayed({ timeout: 3_000 });

        // Cancel — must not reset
        const cancelBtn = await $('button*=Cancel');
        await cancelBtn.click();
        await confirmBtn.waitForDisplayed({ timeout: 3_000, reverse: true });

        // Settings should still be showing
        expect((await $$('.section')).length).toBeGreaterThan(0);
    });

    it('Check Now button triggers update check without crashing', async () => {
        const checkBtn = await $('button*=Check Now');
        await checkBtn.waitForClickable({ timeout: 3_000 });
        await checkBtn.click();

        // Button should briefly show "Checking…"
        // Just wait for it to return to normal state (not stuck spinning)
        await browser.waitUntil(
            async () => (await checkBtn.getText()).includes('Check Now'),
            { timeout: 15_000, timeoutMsg: 'Check Now button did not return to normal state' }
        );

        // No error should have appeared
        const errorBox = await $('.error-box');
        if (await errorBox.isExisting()) {
            const msg = await errorBox.getText();
            // Update check errors are non-fatal (network may be unavailable) — log but don't fail
            console.warn(`Update check returned error (network may be down): ${msg}`);
        }
    });
});
