// Tests for backuppilot-cli: list, snapshots, archives, files, restore-run.
// These tests run shell commands via child_process — they do not interact with the
// browser but share the wdio session so the Tauri app (and daemon) are alive
// alongside the CLI, exercising the concurrent-reader path on the SQLite DB.

import { execSync } from 'child_process';

const CLI = 'C:\\Users\\user\\backuppilot\\target\\release\\backuppilot-cli.exe';
const PROFILE = 'Windows-Pictures-E2E';
const CLI_RESTORE_TARGET = 'C:\\Users\\user\\Pictures\\TestRestoreCli';

function runCli(args: string): string {
    return execSync(`"${CLI}" ${args}`, { encoding: 'utf8', timeout: 120_000 });
}

// Discovered once in 'snapshots' test, reused in archives / files / restore-run.
let snapshotPath = '';
let archiveName = '';

describe('backuppilot-cli', () => {
    before(async () => {
        // Navigate so the wdio session has an active page; CLI tests themselves use execSync.
        await browser.url('http://tauri.localhost/');
        await browser.waitUntil(
            async () => {
                const html = await browser.execute(() => document.body.innerHTML) as string;
                return html.length > 200;
            },
            { timeout: 30_000, timeoutMsg: 'App body remained empty' }
        );
    });

    it('list includes the Windows-Pictures-E2E profile', () => {
        const output = runCli('--json list');
        const profiles = JSON.parse(output) as Array<{ name: string }>;
        const found = profiles.some(p => p.name === PROFILE);
        expect(found).toBe(true);
    });

    it('snapshots returns at least one snapshot for the profile', () => {
        const output = runCli(`--json snapshots "${PROFILE}"`);
        const snapshots = JSON.parse(output) as Array<{ path: string; archives: string[] }>;
        expect(snapshots.length).toBeGreaterThan(0);
        snapshotPath = snapshots[0].path;
        archiveName = snapshots[0].archives[0] ?? '';
    });

    it('archives returns archive list for the first snapshot', () => {
        expect(snapshotPath).toBeTruthy();
        const output = runCli(`--json archives "${PROFILE}" "${snapshotPath}"`);
        const result = JSON.parse(output) as { archives: string[] };
        expect(result.archives.length).toBeGreaterThan(0);
        archiveName = result.archives[0];
    });

    it('files returns entries at the archive root', () => {
        expect(snapshotPath).toBeTruthy();
        expect(archiveName).toBeTruthy();
        const output = runCli(`--json files "${PROFILE}" "${snapshotPath}" "${archiveName}"`);
        const result = JSON.parse(output) as {
            entries: Array<{ name: string; path: string; is_dir: boolean }>;
        };
        expect(result.entries.length).toBeGreaterThan(0);
    });

    it('restore-run restores a single file using --pattern', () => {
        expect(snapshotPath).toBeTruthy();
        expect(archiveName).toBeTruthy();

        // Pick the first non-directory entry as the restore pattern.
        const filesOutput = runCli(`--json files "${PROFILE}" "${snapshotPath}" "${archiveName}"`);
        const catalog = JSON.parse(filesOutput) as {
            entries: Array<{ name: string; path: string; is_dir: boolean }>;
        };
        const file = catalog.entries.find(e => !e.is_dir);
        expect(file).toBeDefined();

        const pattern = file!.name;
        const output = runCli(
            `--json restore-run "${PROFILE}" "${snapshotPath}" "${archiveName}" ` +
            `"${CLI_RESTORE_TARGET}" --pattern "${pattern}" --overwrite`
        );
        const result = JSON.parse(output) as { ok: boolean; message?: string; conflicts?: string[] };
        if (!result.ok) {
            throw new Error(`restore-run failed: ${result.message}`);
        }
        expect(result.ok).toBe(true);
    });
});
