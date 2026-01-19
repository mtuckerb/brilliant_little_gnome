const { app, BrowserWindow, ipcMain, shell, dialog, protocol, net } = require('electron');
const path = require('path');
const { spawn, execSync, exec } = require('child_process');
const axios = require('axios');
const fs = require('fs');

let mainWindow;
let rubyApp;

// Configuration based on app package identity
const appName = "brilliant";
const productName = "Brilliant";

// Rebranding: Transition operational directory from "brightspace" to "Brilliant"
// We use app.getPath('userData') which Electron handles based on the 'name' in package.json
const userDataPath = app.getPath('userData');
const logFile = path.join(userDataPath, 'sidecar.log');

// Ensure data directory exists
if (!fs.existsSync(userDataPath)) {
  fs.mkdirSync(userDataPath, { recursive: true });
}

function getRubyBinary() {
  if (process.platform === 'darwin') {
    try {
      // Try to find ruby via which, fall back to common path
      const pathFound = execSync('which ruby').toString().trim();
      return pathFound || '/usr/bin/ruby';
    } catch (e) {
      return '/usr/bin/ruby';
    }
  }
  return 'ruby'; // Assume it's in PATH for other platforms
}

function startRubyApp() {
  const rubyBinary = getRubyBinary();
  const baseDir = app.getAppPath();
  
  // Set up environment for Ruby
  const env = { ...process.env };
  
  // Set the data directory so Ruby knows where to store its database and config
  env.BRILLIANT_DATA_DIR = userDataPath;
  
  // In development, we might need to point to the local lib
  env.RUBYLIB = path.join(baseDir, 'lib') + (env.RUBYLIB ? `:${env.RUBYLIB}` : '');
  
  // Point to the gems bundled with the app if we're in a packaged state
  // This depends on how you package the ruby environment.
  // For now, assume a local environment where gems are available.

  let logFd;
  try {
    logFd = fs.openSync(logFile, 'a');
    const startupMsg = `
--- Startup at ${new Date().toISOString()} ---
Platform: ${process.platform} (${process.arch})
App Path: ${app.getAppPath()}
Base Dir: ${baseDir}
Ruby Bin: ${rubyBinary}
Data Dir: ${userDataPath}
RUBYLIB: ${env.RUBYLIB}
GEM_PATH: ${env.GEM_PATH}
---------------------------
`;
    fs.writeSync(logFd, startupMsg);
  } catch (err) {
    console.error("Failed to open log file:", err);
  }

  // Forward command line arguments (e.g., --headless) to Ruby
  const rubyArgs = ['app.rb', ...process.argv.slice(2)];

  rubyApp = spawn(rubyBinary, rubyArgs, {
    cwd: baseDir,
    env: env,
    stdio: ['ignore', 'pipe', 'pipe']
  });

  // Pipe Ruby output to both the log file and the Electron process console
  // This ensures metadata (DB path, URLs) shows up during 'npm start'
  if (rubyApp.stdout) {
    rubyApp.stdout.on('data', (data) => {
      if (logFd) fs.writeSync(logFd, data);
      process.stdout.write(data);
    });
  }

  if (rubyApp.stderr) {
    rubyApp.stderr.on('data', (data) => {
      if (logFd) fs.writeSync(logFd, data);
      process.stderr.write(data);
    });
  }

  rubyApp.on('error', (err) => {
    console.error(`[Electron] Failed to start Ruby process: ${err}`);
    dialog.showErrorBox("Ruby Startup Error", `Failed to start the Ruby sidecar: ${err.message}\nBinary: ${rubyBinary}`);
  });

  rubyApp.on('exit', (code, signal) => {
    console.log(`[Electron] Ruby sidecar exited with code ${code} and signal ${signal}`);
    if (code !== 0 && code !== null) {
      // Only show error box if not already quitting
      if (!app.isQuitting) {
        dialog.showErrorBox("Ruby Sidecar Crash", `The Ruby application exited unexpectedly (code: ${code}).\nCheck ${logFile} for details.`);
      }
    }
    if (logFd) fs.closeSync(logFd);
  });
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    title: productName,
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      preload: path.join(__dirname, 'preload.js')
    }
  });

  // We wait for the Ruby app to be ready
  const checkRuby = setInterval(async () => {
    try {
      await axios.get('http://localhost:4567/health');
      clearInterval(checkRuby);
      mainWindow.loadURL('http://localhost:4567/');
    } catch (e) {
      // Ruby not ready yet
    }
  }, 500);

  mainWindow.on('closed', function () {
    mainWindow = null;
  });
}

// Check for --headless flag
const isHeadless = process.argv.includes('--headless');

app.whenReady().then(() => {
  startRubyApp();
  if (!isHeadless) {
    createWindow();
  } else {
    console.log("[Electron] Running in headless mode. UI will not be shown.");
  }

  app.on('activate', function () {
    if (mainWindow === null && !isHeadless) createWindow();
  });
});

app.on('window-all-closed', function () {
  // On macOS it is common for applications and their menu bar
  // to stay active until the user quits explicitly with Cmd + Q
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

// Flag to indicate we are intentionally shutting down
app.isQuitting = false;

app.on('before-quit', () => {
  app.isQuitting = true;
  if (rubyApp) {
    console.log("[Electron] Sending TERM to Ruby sidecar...");
    rubyApp.kill('SIGTERM');
  }
});

// Handle cookie requests from the sidecar
ipcMain.handle('get-cookies', async (event, url) => {
  const cookies = await mainWindow.webContents.session.cookies.get({ url: url });
  return cookies;
});

// Handle open-url requests
ipcMain.on('open-external', (event, url) => {
  shell.openExternal(url);
});
