// Activity page UI tests via WebdriverIO + tauri-driver

describe('Activity page', () => {
    before(async () => {
        await browser.url('http://tauri.localhost/');
        await browser.waitUntil(
            async () => {
                const html = await browser.execute(() => document.body.innerHTML) as string;
                return html.length > 200;
            },
            { timeout: 30_000, timeoutMsg: 'App body remained empty after navigation' }
        );

        // The app opens on the Profiles page. Wait for actual Profiles content to appear
        // (a profile card, empty-state div, or error box) rather than waiting for the
        // spinner to disappear — the spinner may not have rendered yet when we first check.
        // This confirms the daemon completed at least one IPC round-trip before we
        // navigate to Activity.
        await browser.waitUntil(
            async () => {
                return (await $('.profile-card').isExisting())
                    || (await $('.empty').isExisting())
                    || (await $('.error-box').isExisting());
            },
            { timeout: 90_000, timeoutMsg: 'Profiles page did not show content (daemon not ready)' }
        );

        const activityTab = await $('.nav-item*=Activity');
        await activityTab.waitForClickable({ timeout: 10_000 });
        await activityTab.click();
        await browser.waitUntil(
            async () => !(await $('.spinner').isExisting()),
            { timeout: 15_000, timeoutMsg: 'Activity page spinner did not clear' }
        );
        await browser.pause(300);
    });

    it('renders the filters bar', async () => {
        const filters = await $('.filters');
        await filters.waitForDisplayed({ timeout: 5_000 });
        expect(await filters.isDisplayed()).toBe(true);

        // Both status and profile dropdowns must be present
        const selects = await filters.$$('select');
        expect(selects.length).toBeGreaterThanOrEqual(2);
    });

    it('shows activity entries or empty state — no error', async () => {
        const errorBox = await $('.error-box');
        if (await errorBox.isExisting()) {
            const msg = await errorBox.getText();
            throw new Error(`Activity page shows error: ${msg}`);
        }

        const hasRows  = (await $$('.log-row')).length > 0;
        const hasEmpty = await $('.empty').isExisting();
        expect(hasRows || hasEmpty).toBe(true);
    });

    it('each log row has a non-empty profile name and status badge', async () => {
        const rows = await $$('.log-row');
        if (rows.length === 0) {
            // No entries — skip assertion (empty state is valid)
            return;
        }

        for (const row of rows.slice(0, 5)) { // check first 5 to keep test fast
            const profile = await row.$('.log-profile');
            const badge   = await row.$('.badge');

            expect(await profile.isExisting()).toBe(true);
            const profileText = await profile.getText();
            expect(profileText.trim()).not.toBe('');
            expect(profileText.trim()).not.toBe('—');

            expect(await badge.isExisting()).toBe(true);
            const badgeText = await badge.getText();
            const validStatuses = ['success', 'failed', 'skipped', 'running', 'cancelled'];
            expect(validStatuses).toContain(badgeText.trim().toLowerCase());
        }
    });

    it('clicking a row expands the detail panel', async () => {
        const rows = await $$('.log-row');
        if (rows.length === 0) return; // no entries to click

        await rows[0].click();
        const detail = await $('.detail-panel');
        await detail.waitForDisplayed({ timeout: 3_000 });
        expect(await detail.isDisplayed()).toBe(true);

        // Detail panel should show at least Profile and Status labels
        const text = await detail.getText();
        expect(text.toUpperCase()).toMatch(/PROFILE|STATUS/);

        // Clicking the same row again collapses the panel
        await rows[0].click();
        await detail.waitForDisplayed({ timeout: 3_000, reverse: true });
    });

    it('status filter hides non-matching rows', async () => {
        const rows = await $$('.log-row');
        if (rows.length === 0) return;

        // Pick the status of the first row to filter by.
        // Badge text may be upper/lowercase; option labels in the select are Title Case.
        const firstBadge = await rows[0].$('.badge');
        const rawStatus = (await firstBadge.getText()).trim();
        const titleStatus = rawStatus.charAt(0).toUpperCase() + rawStatus.slice(1).toLowerCase();

        const statusSelect = await $$('.filters select')[1];
        await statusSelect.selectByVisibleText(titleStatus);
        await browser.pause(200);

        const filtered = await $$('.log-row');
        for (const row of filtered) {
            const badge = await row.$('.badge');
            const text = (await badge.getText()).trim().toLowerCase();
            expect(text).toBe(rawStatus.toLowerCase());
        }

        // Reset filter
        await statusSelect.selectByVisibleText('All statuses');
    });

    it('search filter narrows the list', async () => {
        const rows = await $$('.log-row');
        if (rows.length === 0) return;

        // Use the profile name from the first row as search term
        const firstProfile = await rows[0].$('.log-profile');
        const profileName  = (await firstProfile.getText()).trim();
        if (!profileName || profileName === '—') return;

        const searchInput = await $('.search-input');
        await searchInput.setValue(profileName);
        await browser.pause(200);

        const filtered = await $$('.log-row');
        expect(filtered.length).toBeGreaterThan(0);
        for (const row of filtered) {
            const text = await row.getText();
            expect(text.toLowerCase()).toContain(profileName.toLowerCase());
        }

        // Clear search
        await searchInput.clearValue();
    });
});
