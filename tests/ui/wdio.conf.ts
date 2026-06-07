import type { Options } from '@wdio/types';

// tauri-driver runs on Windows and listens on port 4444.
// We connect via SSH tunnel: ssh -L 4444:localhost:4444 user@10.99.158.244
const TAURI_APP = process.env.TAURI_APP_PATH
    || 'C:\\Users\\user\\backuppilot\\target\\release\\backuppilot-tauri.exe';

export const config: Options.Testrunner = {
    runner: 'local',
    hostname: 'localhost',
    port: 4444,
    path: '/',
    specs: ['./specs/**/*.spec.ts'],
    maxInstances: 1,
    // Run specs sequentially — two parallel sessions would both launch the Tauri app
    // via tauri-driver, conflicting on the named pipe and WebView2 instance.
    specFileRetries: 0,
    capabilities: [{
        browserName: 'chrome',
        'tauri:options': {
            application: TAURI_APP,
        },
        // msedgedriver creates WebView2 temp dirs under the TEMP env var.
        // In SSH sessions TEMP may point to C:\WINDOWS\SystemTemp (not user-writable),
        // so we force a specific writable location here.
        'ms:edgeOptions': {
            args: [
                '--user-data-dir=C:\\Users\\user\\AppData\\Local\\Temp\\backuppilot-wdio',
            ],
        },
    }] as any[],
    logLevel: 'info',
    bail: 0,
    waitforTimeout: 20_000,
    connectionRetryTimeout: 120_000,
    connectionRetryCount: 3,
    framework: 'mocha',
    reporters: ['spec'],
    mochaOpts: {
        // 3 minutes: cold WSL probe on first spec can take ~30s, and some restore/catalog
        // waitUntil calls are set to 90–120s; the mocha timeout must exceed the longest chain.
        timeout: 180_000,
    },
};
