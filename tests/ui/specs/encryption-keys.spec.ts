// Encryption Keys page UI tests via WebdriverIO + tauri-driver

describe('Encryption Keys page', () => {
    before(async () => {
        await browser.url('http://tauri.localhost/');
        await browser.waitUntil(
            async () => {
                const html = await browser.execute(() => document.body.innerHTML) as string;
                return html.length > 200;
            },
            { timeout: 30_000, timeoutMsg: 'App body remained empty after navigation' }
        );
        const keysTab = await $('.nav-item*=Encryption Keys');
        await keysTab.waitForClickable({ timeout: 10_000 });
        await keysTab.click();
        await browser.waitUntil(
            async () => !(await $('.spinner').isExisting()),
            { timeout: 15_000, timeoutMsg: 'Encryption Keys spinner did not clear' }
        );
        await browser.pause(300);

        // Clean up any stale UI-Test-Key-* entries left by previous runs.
        // Re-query rows each iteration: delete reloads the list and old references go stale.
        let staleRows = await $$('.key-row');
        for (const row of staleRows) {
            const nameEl = await row.$('.key-name');
            if (!(await nameEl.getText()).startsWith('UI-Test-Key-')) continue;
            const deleteBtn = await row.$('.btn-danger-sm');
            if (!(await deleteBtn.isExisting())) continue;
            await deleteBtn.click();
            const confirmBtn = await $('button*=Delete Key');
            if (await confirmBtn.isExisting()) await confirmBtn.click();
            // Wait for the dialog to close and the list to reload before continuing
            await browser.waitUntil(
                async () => !(await $('.dialog').isExisting()),
                { timeout: 10_000, timeoutMsg: 'Delete dialog did not close' }
            );
            await browser.pause(300);
        }
    });

    it('shows the warning callout', async () => {
        const callout = await $('.warning-callout');
        await callout.waitForDisplayed({ timeout: 5_000 });
        const text = await callout.getText();
        expect(text).toMatch(/key|backup/i);
    });

    it('shows keys list or empty state — no error', async () => {
        const errorBox = await $('.error-box');
        if (await errorBox.isExisting()) {
            const msg = await errorBox.getText();
            throw new Error(`Encryption Keys page shows error: ${msg}`);
        }
        const hasRows  = (await $$('.key-row')).length > 0;
        const hasEmpty = await $('.empty').isExisting();
        expect(hasRows || hasEmpty).toBe(true);
    });

    it('each key row has a non-empty name and creation date', async () => {
        const rows = await $$('.key-row');
        for (const row of rows) {
            const name = await (await row.$('.key-name')).getText();
            const meta = await (await row.$('.key-meta')).getText();
            expect(name.trim()).not.toBe('');
            expect(meta.trim()).not.toBe('');
            expect(meta.trim()).not.toBe('—');
        }
    });

    it('opens create form when "+ New Key" is clicked', async () => {
        const newKeyBtn = await $('button*=New Key');
        await newKeyBtn.waitForClickable({ timeout: 5_000 });
        await newKeyBtn.click();

        const form = await $('.create-form');
        await form.waitForDisplayed({ timeout: 3_000 });
        expect(await form.isDisplayed()).toBe(true);

        // Cancel button closes the form
        const cancelBtn = await form.$('button*=Cancel');
        await cancelBtn.click();
        await form.waitForDisplayed({ timeout: 3_000, reverse: true });
    });

    it('Create Key button is disabled when name is empty', async () => {
        const newKeyBtn = await $('button*=New Key');
        await newKeyBtn.click();
        const form = await $('.create-form');
        await form.waitForDisplayed({ timeout: 3_000 });

        const input = await $('#key-name');
        await input.clearValue();

        const createBtn = await $('button*=Create Key');
        expect(await createBtn.getAttribute('disabled')).not.toBeNull();

        await (await form.$('button*=Cancel')).click();
        await form.waitForDisplayed({ timeout: 3_000, reverse: true });
    });

    it('creates a test key and shows it in the list', async () => {
        const testName = `UI-Test-Key-${Date.now()}`;
        const countBefore = (await $$('.key-row')).length;

        const newKeyBtn = await $('button*=New Key');
        await newKeyBtn.click();
        const form = await $('.create-form');
        await form.waitForDisplayed({ timeout: 3_000 });

        await $('#key-name').setValue(testName);
        await $('button*=Create Key').click();

        // Capture any error shown by the UI to surface it in the test failure message
        await browser.pause(2_000);
        const errBox = await $('.error-box');
        if (await errBox.isExisting()) {
            const msg = await errBox.getText();
            throw new Error(`Create key failed with UI error: ${msg}`);
        }

        // Form closes and new row appears.
        // Allow 30s: WSL cold-start on first invocation can take 10-15s.
        await form.waitForDisplayed({ timeout: 30_000, reverse: true });
        await browser.waitUntil(
            async () => (await $$('.key-row')).length > countBefore,
            { timeout: 10_000, timeoutMsg: `Key row for "${testName}" did not appear` }
        );

        let found = false;
        for (const row of await $$('.key-row')) {
            if ((await (await row.$('.key-name')).getText()) === testName) {
                found = true;
                break;
            }
        }
        expect(found).toBe(true);
    });

    it('delete confirmation dialog appears and cancels correctly', async () => {
        // Find the test key row created above
        let testRow = null;
        for (const row of await $$('.key-row')) {
            if ((await (await row.$('.key-name')).getText()).startsWith('UI-Test-Key-')) {
                testRow = row;
                break;
            }
        }
        if (!testRow) throw new Error('Test key row not found');

        await (await testRow.$('.btn-danger-sm')).click();

        const dialog = await $('.dialog');
        await dialog.waitForDisplayed({ timeout: 3_000 });
        const dialogText = await dialog.getText();
        expect(dialogText).toMatch(/delete/i);

        // Cancel — key must still exist
        await (await dialog.$('button*=Cancel')).click();
        await dialog.waitForDisplayed({ timeout: 3_000, reverse: true });

        let stillExists = false;
        for (const row of await $$('.key-row')) {
            if ((await (await row.$('.key-name')).getText()).startsWith('UI-Test-Key-')) {
                stillExists = true;
                break;
            }
        }
        expect(stillExists).toBe(true);
    });

    it('confirms deletion and removes the test key', async () => {
        let testRow = null;
        for (const row of await $$('.key-row')) {
            if ((await (await row.$('.key-name')).getText()).startsWith('UI-Test-Key-')) {
                testRow = row;
                break;
            }
        }
        if (!testRow) throw new Error('Test key row not found');
        const testName = await (await testRow.$('.key-name')).getText();

        await (await testRow.$('.btn-danger-sm')).click();
        const dialog = await $('.dialog');
        await dialog.waitForDisplayed({ timeout: 3_000 });
        await (await dialog.$('button*=Delete Key')).click();
        await dialog.waitForDisplayed({ timeout: 5_000, reverse: true });

        await browser.waitUntil(
            async () => {
                for (const row of await $$('.key-row')) {
                    if ((await (await row.$('.key-name')).getText()) === testName) return false;
                }
                return true;
            },
            { timeout: 10_000, timeoutMsg: `Key "${testName}" still present after deletion` }
        );
    });
});
