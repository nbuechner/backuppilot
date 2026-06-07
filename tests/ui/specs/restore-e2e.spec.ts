// End-to-end restore lifecycle test.
// Depends on profiles-e2e.spec.ts having run first (snapshot of Windows-Pictures-E2E exists).

const PROFILE_NAME = 'Windows-Pictures-E2E';
const RESTORE_TARGET = 'C:\\Users\\user\\Pictures\\TestRestore';

async function navigateTo(tabLabel: string) {
    const tab = await $(`.nav-item*=${tabLabel}`);
    await tab.waitForClickable({ timeout: 10_000 });
    await tab.click();
    await browser.waitUntil(
        async () => !(await $('.spinner').isExisting()),
        { timeout: 15_000, timeoutMsg: `${tabLabel} spinner did not clear` }
    );
    await browser.pause(300);
}

describe('Restore lifecycle', () => {
    before(async () => {
        await browser.url('http://tauri.localhost/');
        await browser.waitUntil(
            async () => {
                const html = await browser.execute(() => document.body.innerHTML) as string;
                return html.length > 200;
            },
            { timeout: 30_000, timeoutMsg: 'App body remained empty' }
        );
        await navigateTo('Restore');
    });

    // ── 1. Select profile ─────────────────────────────────────────────────────

    it('finds the Windows-Pictures-E2E profile in the restore list', async () => {
        await browser.waitUntil(
            async () => (await $$('.list-item')).length > 0,
            { timeout: 15_000, timeoutMsg: 'No profiles appeared on Restore page' }
        );

        let found = false;
        for (const item of await $$('.list-item')) {
            if ((await item.getText()).includes(PROFILE_NAME)) {
                found = true;
                break;
            }
        }
        expect(found).toBe(true);
    });

    it('selecting the profile loads at least one snapshot', async () => {
        for (const item of await $$('.list-item')) {
            if ((await item.getText()).includes(PROFILE_NAME)) {
                await item.click();
                break;
            }
        }

        await browser.waitUntil(
            async () => (await $$('.snapshot-item')).length > 0,
            { timeout: 60_000, timeoutMsg: 'No snapshots loaded after selecting profile' }
        );

        const count = (await $$('.snapshot-item')).length;
        expect(count).toBeGreaterThan(0);
    });

    // ── 2. Select snapshot, browse catalog ───────────────────────────────────

    it('selecting the most recent snapshot loads the catalog', async () => {
        const firstSnap = await $('.snapshot-item');
        await firstSnap.click();

        await browser.waitUntil(
            async () => {
                const err = await $('.error-box');
                if (await err.isExisting()) {
                    throw new Error(`Catalog error: ${await err.getText()}`);
                }
                return (await $$('.file-row')).length > 0;
            },
            { timeout: 90_000, timeoutMsg: 'Catalog did not load after selecting snapshot (90s)' }
        );

        const entries = await $$('.file-row');
        expect(entries.length).toBeGreaterThan(0);
    });

    // ── 3. Restore entire archive to a target dir ────────────────────────────

    it('restores the full archive to a target directory', async () => {
        const targetInput = await $('#target-dir');
        await targetInput.waitForDisplayed({ timeout: 5_000 });
        await targetInput.setValue(RESTORE_TARGET);

        // Enable overwrite so re-runs don't fail on existing files
        const overwriteCheckbox = await $('.restore-options .toggle-row input[type="checkbox"]');
        const isChecked = await overwriteCheckbox.isSelected();
        if (!isChecked) await overwriteCheckbox.click();

        const restoreBtn = await $('button*=Start Restore');
        await restoreBtn.waitForClickable({ timeout: 5_000 });
        await restoreBtn.click();

        // Wait for restore to complete — can take a while for large archives
        await browser.waitUntil(
            async () => {
                const ok = await $('.result-ok');
                if (await ok.isExisting()) return true;
                const err = await $('.result-error');
                if (await err.isExisting()) {
                    throw new Error(`Restore failed: ${await err.getText()}`);
                }
                return false;
            },
            { timeout: 120_000, timeoutMsg: 'Restore did not complete within 2 minutes' }
        );

        const resultText = await $('.result-ok').getText();
        expect(resultText).toContain('successfully');
    });

    // ── 4. Restore to a different location ───────────────────────────────────

    it('restores to a second location (alternate target dir)', async () => {
        // Navigate away and back to reset the UI state
        await navigateTo('Profiles');
        await navigateTo('Restore');

        // Re-select profile
        await browser.waitUntil(
            async () => (await $$('.list-item')).length > 0,
            { timeout: 15_000, timeoutMsg: 'Profile list did not reload' }
        );
        for (const item of await $$('.list-item')) {
            if ((await item.getText()).includes(PROFILE_NAME)) {
                await item.click();
                break;
            }
        }

        // Re-select snapshot
        await browser.waitUntil(
            async () => (await $$('.snapshot-item')).length > 0,
            { timeout: 60_000, timeoutMsg: 'Snapshots did not reload' }
        );
        await (await $('.snapshot-item')).click();

        // Wait for catalog
        await browser.waitUntil(
            async () => (await $$('.file-row')).length > 0,
            { timeout: 90_000, timeoutMsg: 'Catalog did not reload' }
        );

        const altTarget = RESTORE_TARGET + '-alt';
        const targetInput = await $('#target-dir');
        await targetInput.waitForDisplayed({ timeout: 5_000 });
        await targetInput.setValue(altTarget);

        const overwriteCheckbox = await $('.restore-options .toggle-row input[type="checkbox"]');
        const isChecked = await overwriteCheckbox.isSelected();
        if (!isChecked) await overwriteCheckbox.click();

        const restoreBtn = await $('button*=Start Restore');
        await restoreBtn.waitForClickable({ timeout: 5_000 });
        await restoreBtn.click();

        await browser.waitUntil(
            async () => {
                const ok = await $('.result-ok');
                if (await ok.isExisting()) return true;
                const err = await $('.result-error');
                if (await err.isExisting()) {
                    throw new Error(`Restore to alt location failed: ${await err.getText()}`);
                }
                return false;
            },
            { timeout: 120_000, timeoutMsg: 'Alt-location restore did not complete within 2 minutes' }
        );

        expect(await (await $('.result-ok')).getText()).toContain('successfully');
    });
});
