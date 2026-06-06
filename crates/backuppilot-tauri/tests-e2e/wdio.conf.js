const { spawn } = require('child_process')
const path = require('path')
const os = require('os')

const isWindows = os.platform() === 'win32'

// Path to the compiled debug binary (run `cargo build -p backuppilot-tauri` first)
const appBin = path.resolve(
  __dirname, '..', '..', '..', 'target', 'debug',
  'backuppilot-tauri' + (isWindows ? '.exe' : '')
)

// tauri-driver process handle
let tauriDriver

// Ensure cleanup on unexpected exit
process.on('exit', () => { if (tauriDriver) tauriDriver.kill() })

exports.config = {
  specs: ['./test/specs/**/*.js'],
  maxInstances: 1,

  capabilities: [{
    maxInstances: 1,
    'tauri:options': { application: appBin },
  }],

  logLevel: 'warn',
  reporters: ['spec'],
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    timeout: 30000,
  },

  // Spawn tauri-driver before tests.
  // Prerequisites:
  //   cargo install tauri-driver
  //   Windows: msedgedriver.exe on PATH matching your WebView2/Edge version
  //            (download from https://developer.microsoft.com/en-us/microsoft-edge/tools/webdriver/)
  //   Linux/macOS: WebKitWebDriver on PATH
  onPrepare: () => {
    tauriDriver = spawn('tauri-driver', [], {
      stdio: [null, process.stdout, process.stderr],
    })
  },

  onComplete: () => {
    if (tauriDriver) tauriDriver.kill()
  },
}
