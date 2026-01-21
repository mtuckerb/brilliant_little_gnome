const { app, BrowserWindow, ipcMain, session, dialog, shell } = require('electron');
const { spawn, execSync } = require('child_process');
const path = require('path');
const axios = require('axios');
const fs = require('fs');

let mainWindow;
let rubyApp;

const userDataPath = app.getPath('userData');
const logFile = path.join(userDataPath, 'ruby_sidecar.log');
const dbDir = path.join(userDataPath, 'db');
const cacheDir = path.join(userDataPath, 'cache');

// Determine resource paths for packaged vs dev
const isPackaged = app.isPackaged;
const resourceDir = isPackaged 
  ? process.resourcesPath 
  : __dirname;

const baseDir = isPackaged
  ? path.join(resourceDir, 'app.asar.unpacked')
  : __dirname;

function getRubyBinary() {
  const arch = process.arch === 'arm64' ? 'arm64' : 'x64';
  const platformDir = process.platform === 'darwin' ? `macos-${arch}` : 'win-x64';
  const rubyExec = process.platform === 'win32' ? 'ruby.exe' : 'ruby';
  
  // Production path in packaged app
  const prodPath = path.join(resourceDir, 'bin', 'ruby_dist', platformDir, 'bin', rubyExec);
  
  if (fs.existsSync(prodPath)) {
    return prodPath;
  }

  // Fallback for development
  if (process.platform === 'darwin') {
    try {
      return execSync('which ruby').toString().trim() || '/usr/bin/ruby';
    } catch (e) {
      return '/usr/bin/ruby';
    }
  }
  return 'ruby';
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1400,
    height: 900,
    title: "Brilliant",
    backgroundColor: '#ffffff',
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      preload: path.join(__dirname, 'preload.js')
    }
  });

  // Load a placeholder while waiting
  mainWindow.loadFile(path.join(__dirname, 'public', 'splash.html')).catch(() => {});

  // Attempt to load the URL with retries
  let retryCount = 0;
  const healthUrl = 'http://127.0.0.1:4567/health';
  
  const loadWithRetry = () => {
    retryCount++;
    if (retryCount % 10 === 0) {
      console.log(`[Electron] Still waiting for Ruby sidecar at ${healthUrl} (Attempt ${retryCount})...`);
    }
    
    if (retryCount > 60) { 
      console.log("[Electron] Timeout reached. Sidecar may have failed to start.");
      if (isPackaged) {
          dialog.showErrorBox("Startup Error", "The Brilliant backend failed to respond in time.");
      }
      return;
    }

    axios.get(healthUrl, { timeout: 1000 })
      .then(res => {
        if (res.status === 200) {
          console.log("[Electron] Ruby sidecar is healthy! Loading dashboard.");
          mainWindow.loadURL('http://127.0.0.1:4567');
        } else {
          setTimeout(loadWithRetry, 500);
        }
      })
      .catch(() => {
        setTimeout(loadWithRetry, 500);
      });
  };

  loadWithRetry();

  mainWindow.on('closed', function () {
    mainWindow = null;
  });

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (url.includes('/download') || url.startsWith('http://127.0.0.1') || url.startsWith('http://localhost')) {
      return { action: 'allow' };
    }
    if (url.startsWith('http')) {
      shell.openExternal(url).catch(err => console.error(`[Electron] Failed to open external link: ${url}`, err));
      return { action: 'deny' };
    }
    return { action: 'allow' };
  });

  mainWindow.webContents.on('will-navigate', (event, url) => {
    if (url.startsWith('http://127.0.0.1') || url.startsWith('http://localhost') || url.includes('/download')) {
      return;
    }
    if (url.startsWith('http')) {
      event.preventDefault();
      shell.openExternal(url).catch(err => console.error(`[Electron] Failed to open external link: ${url}`, err));
    }
  });
}

function startRubyApp() {
  const rubyBinary = getRubyBinary();
  const pidFile = path.join(userDataPath, 'ruby_sidecar.pid');

  if (!fs.existsSync(userDataPath)) fs.mkdirSync(userDataPath, { recursive: true });
  if (!fs.existsSync(dbDir)) fs.mkdirSync(dbDir, { recursive: true });
  if (!fs.existsSync(cacheDir)) fs.mkdirSync(cacheDir, { recursive: true });

  if (fs.existsSync(pidFile)) {
    try {
      const oldPid = parseInt(fs.readFileSync(pidFile, 'utf8'));
      if (oldPid) process.kill(oldPid, 'SIGTERM');
    } catch (e) {}
    try { fs.unlinkSync(pidFile); } catch(e) {}
  }

  try { 
    if (process.platform !== 'win32') fs.chmodSync(rubyBinary, 0o755); 
  } catch(e) {}

  const arch = process.arch === 'arm64' ? 'arm64' : 'x64';
  const platformDir = process.platform === 'darwin' ? `macos-${arch}` : 'win-x64';
  const rubyBase = path.join(resourceDir, 'bin', 'ruby_dist', platformDir);
  const vendorGems = path.join(resourceDir, 'vendor', 'bundle', 'ruby', '3.4.0');
  const internalGems = path.join(rubyBase, 'lib', 'ruby', 'gems', '3.4.0');
  const pathSeparator = process.platform === 'win32' ? ';' : ':';

  // Build a robust environment for Ruby
  const env = { 
    ...process.env, 
    PORT: '4567', 
    BUNDLE_GEMFILE: path.join(baseDir, 'Gemfile'),
    BUNDLE_DEPLOYMENT: 'true', 
    BUNDLE_PATH: path.join(resourceDir, 'vendor', 'bundle'),
    GEM_PATH: `${vendorGems}${pathSeparator}${internalGems}`,
    GEM_HOME: vendorGems,
    RUBY_PLATFORM_DIR: platformDir,
    BRILLIANT_DATA_DIR: userDataPath,
    BRILLIANT_ENV: 'electron',
    BOOTSNAP_CACHE_DIR: cacheDir,
    DATABASE_URL: `sqlite3:///${path.join(dbDir, 'production.sqlite3').replace(/\\/g, '/').replace(/ /g, '%20')}`,
    PATH: `${path.join(rubyBase, 'bin')}${pathSeparator}${process.env.PATH}`,
    // Critical: ignore any global system ruby configurations
    RUBYOPT: "", 
    RUBYLIB: "",
    // Diagnostic logging
    DEBUG: "true"
  };

  // On macOS, some native extensions might need help finding our distributed libraries 
  // if they didn't respect @rpath during build.
  if (process.platform === 'darwin') {
    env.DYLD_FALLBACK_LIBRARY_PATH = `${path.join(rubyBase, 'lib')}${pathSeparator}${process.env.DYLD_FALLBACK_LIBRARY_PATH || ''}`;
  }

  console.log(`[Electron] Spawning Ruby: ${rubyBinary}`);
  
  let logFd;
  try {
    logFd = fs.openSync(logFile, 'a');
    fs.writeSync(logFd, `\n--- SIDE CAR STARTUP AT ${new Date().toISOString()} ---\n`);
    fs.writeSync(logFd, `Binary: ${rubyBinary}\n`);
    fs.writeSync(logFd, `CWD: ${baseDir}\n`);
  } catch (err) {
    console.error("Failed to open log file:", err);
  }

  const rubyArgs = ['app.rb', ...process.argv.slice(2)];

  rubyApp = spawn(rubyBinary, rubyArgs, {
    cwd: baseDir,
    env: env,
    stdio: ['ignore', 'pipe', 'pipe']
  });

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
    console.error(`[Electron] Spawn Error: ${err}`);
    if (logFd) fs.writeSync(logFd, `Spawn Error: ${err.message}\n`);
  });

  rubyApp.on('exit', (code, signal) => {
    if (code !== 0 && !app.isQuitting) {
      console.error(`[Electron] Ruby process exited unexpectedly: Code ${code}, Signal ${signal}`);
      if (logFd) fs.writeSync(logFd, `EXITED UNEXPECTEDLY: Code ${code}, Signal ${signal}\n`);
      if (isPackaged) {
        dialog.showErrorBox("Ruby Sidecar Crash", `The Brilliant backend crashed (Exit Code: ${code}).\nCheck the log at: ${logFile}`);
      }
    }
    if (logFd) fs.closeSync(logFd);
  });
}

const isHeadless = process.argv.includes('--headless');

app.whenReady().then(() => {
  startRubyApp();
  if (!isHeadless) {
    createWindow();
  }
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

app.isQuitting = false;
app.on('before-quit', () => {
  app.isQuitting = true;
  if (rubyApp) {
      try { rubyApp.kill('SIGTERM'); } catch (e) {}
  }
});

ipcMain.on('start-login', (event, host) => {
  const loginWindow = new BrowserWindow({
    width: 600,
    height: 800,
    parent: mainWindow,
    modal: true,
    title: "Brilliant Login",
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true
    }
  });

  loginWindow.loadURL(`https://${host}/d2l/lp/auth/login/login.d2l`);

  loginWindow.webContents.on('dom-ready', () => {
    const script = "window.alert = function(){}; window.confirm = function(){return true;}; window.prompt = function(){return null;};";
    loginWindow.webContents.executeJavaScript(script);
  });

  loginWindow.webContents.on('did-navigate', (event, url) => {
    if (url.includes("/d2l/home") || url.includes("/d2l/lp/homepage")) {
      session.defaultSession.cookies.get({ domain: host })
        .then((cookies) => {
          const cookieString = cookies.map(c => `${c.name}=${c.value}`).join('; ');
          mainWindow.webContents.send('login-complete', { host, cookies: cookieString });
          
          setTimeout(() => {
            loginWindow.close();
          }, 1000);
        })
        .catch((error) => {
          console.error("Failed to get cookies:", error);
        });
    }
  });
});
