// Profiles page UI tests via WebdriverIO + tauri-driver

describe('Profiles page', () => {
    before(async () => {
        await browser.url('http://tauri.localhost/');
        await browser.execute(() => window.location.reload());
        await browser.waitUntil(
            async () => {
                const html = await browser.execute(() => document.body.innerHTML) as string;
                return html.length > 200;
            },
            { timeout: 30_000, timeoutMsg: 'App body remained empty for 30s after navigation' }
        );
        await browser.pause(500);

        // Clean up any stale UI-Test-* profiles left by previous test runs
        for (const card of await $$('.profile-card')) {
            if (!(await card.getText()).includes('UI-Test-')) continue;
            const deleteBtn = await card.$('button*=Delete');
            if (await deleteBtn.isExisting()) {
                await deleteBtn.click();
                const confirmBtn = await $('button*=Confirm, button*=Yes');
                if (await confirmBtn.isExisting()) await confirmBtn.click();
                await browser.pause(300);
            }
        }
    });

    it('loads at least one profile card', async () => {
        await browser.waitUntil(
            async () => (await $$('.profile-card')).length > 0,
            { timeout: 30_000, timeoutMsg: 'No profile cards appeared' }
        );
        const cards = await $$('.profile-card');
        expect(cards.length).toBeGreaterThan(0);
    });

    it('shows PBS host in each profile card (not undefined)', async () => {
        const host = await $('.profile-host');
        await host.waitForDisplayed({ timeout: 10_000 });
        const text = await host.getText();
        expect(text).toBeTruthy();
        expect(text).not.toBe('undefined');
        expect(text).not.toBe('');
    });

    it('opens Add Profile dialog when button clicked', async () => {
        const addBtn = await $('button*=Add Profile');
        await addBtn.waitForClickable({ timeout: 5_000 });
        await addBtn.click();

        const dialog = await $('.dialog, [role="dialog"]');
        await dialog.waitForDisplayed({ timeout: 5_000 });
        expect(await dialog.isDisplayed()).toBe(true);

        await browser.keys('Escape');
        await dialog.waitForDisplayed({ timeout: 5_000, reverse: true });
    });

    it('form shows error when saving with empty required fields', async () => {
        const addBtn = await $('button*=Add Profile');
        await addBtn.click();

        const dialog = await $('.dialog, [role="dialog"]');
        await dialog.waitForDisplayed({ timeout: 5_000 });

        const saveBtn = await $('button*=Create Profile');
        await saveBtn.waitForClickable();
        await saveBtn.click();

        const errorBox = await $('.error-box, .error');
        await errorBox.waitForDisplayed({ timeout: 3_000 });
        const errorText = await errorBox.getText();
        expect(errorText.toLowerCase()).toMatch(/required|name|fill/);

        await browser.keys('Escape');
        await dialog.waitForDisplayed({ timeout: 5_000, reverse: true });
    });

    it('creates and deletes a test profile', async () => {
        const testName = `UI-Test-${Date.now()}`;

        const addBtn = await $('button*=Add Profile');
        await addBtn.click();

        const dialog = await $('.dialog, [role="dialog"]');
        await dialog.waitForDisplayed({ timeout: 5_000 });

        await $('#pf-name').setValue(testName);
        await $('#pf-host').setValue('test-host.local');
        await $('#pf-ds').setValue('test-store');
        await $('#pf-token').setValue('user@pbs!token=00000000-0000-0000-0000-000000000000');

        // Add a backup path (required)
        const pathInput = await $('input[placeholder*="Documents"]');
        await pathInput.setValue('C:\\Users\\user\\Desktop');
        await $('.path-input-row button').click();

        const saveBtn = await $('button*=Create Profile');
        await saveBtn.click();

        await dialog.waitForDisplayed({ timeout: 15_000, reverse: true });
        await browser.waitUntil(
            async () => {
                for (const c of await $$('.profile-card')) {
                    if ((await c.getText()).includes(testName)) return true;
                }
                return false;
            },
            { timeout: 10_000, timeoutMsg: `Profile card "${testName}" did not appear` }
        );

        // Delete the card
        for (const card of await $$('.profile-card')) {
            if ((await card.getText()).includes(testName)) {
                const deleteBtn = await card.$('button*=Delete');
                if (await deleteBtn.isExisting()) {
                    await deleteBtn.click();
                    const confirmBtn = await $('button*=Confirm, button*=Yes');
                    if (await confirmBtn.isExisting()) await confirmBtn.click();
                }
                break;
            }
        }
    });
});
