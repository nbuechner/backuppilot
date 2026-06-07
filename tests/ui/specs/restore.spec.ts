// Restore page UI tests via WebdriverIO + tauri-driver

describe('Restore page', () => {
    before(async () => {
        await browser.url('http://tauri.localhost/');
        await browser.execute(() => window.location.reload());
        await browser.waitUntil(
            async () => {
                const html = await browser.execute(() => document.body.innerHTML) as string;
                return html.length > 200;
            },
            { timeout: 30_000, timeoutMsg: 'App body remained empty after navigation' }
        );
        // Nav items are buttons inside .sidebar with class .nav-item
        const restoreTab = await $('.nav-item*=Restore');
        await restoreTab.waitForClickable({ timeout: 10_000 });
        await restoreTab.click();
        await browser.pause(500);
    });

    it('shows profile list with at least one real (non-test) profile', async () => {
        await browser.waitUntil(
            async () => (await $$('.list-item')).length > 0,
            { timeout: 15_000, timeoutMsg: 'No profile list items appeared on Restore page' }
        );
        // At least one item should not be a UI-Test throwaway profile
        const items = await $$('.list-item');
        let hasReal = false;
        for (const item of items) {
            const text = await item.getText();
            if (!text.startsWith('UI-Test-')) { hasReal = true; break; }
        }
        expect(hasReal).toBe(true);
    });

    it('selecting a profile loads snapshots with real dates', async () => {
        // Find the first non-test profile (UI-Test-* profiles point to fake PBS hosts)
        const items = await $$('.list-item');
        let realProfile = null;
        for (const item of items) {
            if (!(await item.getText()).startsWith('UI-Test-')) {
                realProfile = item;
                break;
            }
        }
        if (!realProfile) throw new Error('No real profile found to test restore');
        await realProfile.click();

        await browser.waitUntil(
            async () => (await $$('.snapshot-item')).length > 0,
            { timeout: 60_000, timeoutMsg: 'No snapshot items appeared after selecting profile' }
        );

        const snapshotCount = (await $$('.snapshot-item')).length;
        expect(snapshotCount).toBeGreaterThan(0);

        // Date should not be "—" (which means parsing failed)
        const dateEl = await $('.snapshot-item .muted, .snapshot-item .date');
        const dateText = await dateEl.getText();
        expect(dateText).not.toBe('—');
        expect(dateText.trim()).not.toBe('');
    });

    it('selecting a snapshot loads the catalog with file entries', async () => {
        const firstSnap = await $('.snapshot-item');
        await firstSnap.click();

        // Catalog dump via WSL can be slow; wait up to 90s.
        // If an error box appears, surface its text in the failure message.
        await browser.waitUntil(
            async () => {
                const errBox = await $('.error-box');
                if (await errBox.isExisting()) {
                    const msg = await errBox.getText();
                    throw new Error(`Catalog load failed with UI error: ${msg}`);
                }
                return (await $$('.file-row, .file-list .entry')).length > 0;
            },
            { timeout: 90_000, timeoutMsg: 'No file entries appeared in catalog after selecting snapshot (90s)' }
        );

        const entries = await $$('.file-row, .file-list .entry');
        expect(entries.length).toBeGreaterThan(0);
    });
});
