const { spawn } = require('child_process')
const path = require('path')
const os = require('os')

const isWindows = os.platform() === 'win32'

// Path to the compiled debug binary (run `cargo build -p backuppilot-tauri` first)
const appBin = path.resolve(
  __dirname, '..', '..', '..', 'target', 'debug',
  'backuppilot-tauri' + (isWindows ? '.exe' : '')
)

let tauriDriver

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

  // Download msedgedriver (Windows) and start tauri-driver before tests.
  // Prerequisites:
  //   cargo install tauri-driver
  //   Linux/macOS: WebKitWebDriver on PATH
  onPrepare: async () => {
    if (isWindows) {
      // Auto-download msedgedriver matching the installed WebView2/Edge version.
      // Reads version from the WebView2 installation directory.
      const { execSync } = require('child_process')
      let edgeVersion
      try {
        edgeVersion = execSync(
          'powershell -Command "(Get-Item \\"C:\\Program Files (x86)\\Microsoft\\EdgeWebView\\Application\\*\\msedgewebview2.exe\\" | Select-Object -First 1).VersionInfo.FileVersion"',
          { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }
        ).trim()
      } catch {
        // Fallback: let edgedriver pick the latest
        edgeVersion = undefined
      }

      const { download } = require('edgedriver')
      const driverPath = await download(edgeVersion)
      // Add the driver directory to PATH so tauri-driver can find it
      process.env.PATH = path.dirname(driverPath) + path.delimiter + process.env.PATH
      console.log(`msedgedriver ready: ${driverPath}`)
    }

    tauriDriver = spawn('tauri-driver', [], {
      stdio: [null, process.stdout, process.stderr],
    })
  },

  onComplete: () => {
    if (tauriDriver) tauriDriver.kill()
  },
}
