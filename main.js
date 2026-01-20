const { app, BrowserWindow, ipcMain, session, dialog, shell } = require('electron');
const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');

let mainWindow;
let rubyApp;

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    title: "Brilliant",
    show: false, // Don't show until ready
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      preload: path.join(__dirname, 'preload.js')
    }
  });

  // Load the splash screen immediately
  mainWindow.loadFile(path.join(__dirname, 'public', 'splash.html'));
  mainWindow.once('ready-to-show', () => {
    mainWindow.show();
  });

  // Attempt to load the URL with retries
  let retryCount = 0;
  const healthUrl = 'http://127.0.0.1:4567/health';
  
  const loadWithRetry = () => {
    retryCount++;
    if (retryCount % 10 === 0) {
      console.log(`[Electron] Still waiting for Ruby sidecar at ${healthUrl} (Attempt ${retryCount})...`);
    }
    
    if (retryCount > 60) { 
      console.log("[Electron] Timeout reached. Opening DevTools for manual inspection.");
      mainWindow.webContents.openDevTools();
    }

    fetch(healthUrl)
      .then(res => {
        if (res.ok) {
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

  // Open dev tools for debugging during development
  // mainWindow.webContents.openDevTools();

  mainWindow.on('closed', function () {
    mainWindow = null;
  });

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    // If it's a download link or local app relative link, allow Electron to handle it
    if (url.includes('/download') || url.startsWith('http://127.0.0.1') || url.startsWith('http://localhost')) {
      return { action: 'allow' };
    }
    
    // For any other external http/https links, open in the system's default browser
    if (url.startsWith('http')) {
      shell.openExternal(url).catch(err => {
        console.error(`[Electron] Failed to open external link: ${url}`, err);
      });
      return { action: 'deny' };
    }

    return { action: 'allow' };
  });

  mainWindow.webContents.on('will-navigate', (event, url) => {
    // If it's the local app or a download, allow it
    if (url.startsWith('http://127.0.0.1') || url.startsWith('http://localhost') || url.includes('/download')) {
      return;
    }

    // Otherwise, intercept and open in external browser
    if (url.startsWith('http')) {
      event.preventDefault();
      shell.openExternal(url).catch(err => {
        console.error(`[Electron] Failed to open external link: ${url}`, err);
      });
    }
  });

  session.defaultSession.on('will-download', (event, item, webContents) => {
    // item.setSavePath(path.join(app.getPath('downloads'), item.getFilename()));
    
    item.on('updated', (event, state) => {
      if (state === 'interrupted') {
        console.log('Download is interrupted but can be resumed');
      } else if (state === 'progressing') {
        if (item.isPaused()) {
          console.log('Download is paused');
        } else {
          console.log(`Received bytes: ${item.getReceivedBytes()}`);
        }
      }
    });
    item.once('done', (event, state) => {
      if (state === 'completed') {
        console.log('Download successfully');
      } else {
        console.log(`Download failed: ${state}`);
      }
    });
  });
}

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

  loginWindow.on('closed', () => {
  });
});

function startRubyApp() {
  const isPackaged = app.isPackaged;
  const baseDir = isPackaged ? app.getAppPath().replace('app.asar', 'app.asar.unpacked') : __dirname;
  const resourceDir = isPackaged ? process.resourcesPath : __dirname;
  const userDataPath = app.getPath('userData') || path.join(app.getPath('appData'), app.getName());
  
  try {
    if (!fs.existsSync(userDataPath)) {
      fs.mkdirSync(userDataPath, { recursive: true });
    }
  } catch (err) {
    console.error(`Failed to create userDataPath: ${userDataPath}`, err);
  }

  const pidFile = path.join(userDataPath, 'ruby_sidecar.pid');

  let platformDir = '';
  let rubyExec = 'ruby';

  if (process.platform === 'darwin') {
    const arch = process.arch === 'arm64' ? 'arm64' : 'x64';
    platformDir = `macos-${arch}`;
  } else if (process.platform === 'win32') {
    platformDir = 'win-x64';
    rubyExec = 'ruby.exe';
  }

  const rubyBase = path.join(resourceDir, 'bin', 'ruby_dist', platformDir);
  const rubyBinary = path.join(rubyBase, 'bin', rubyExec);

  const vendorGems = path.join(resourceDir, 'vendor', 'bundle', 'ruby', '3.4.0');
  const internalGems = path.join(rubyBase, 'lib', 'ruby', 'gems', '3.4.0');
  
  const cacheDir = path.join(userDataPath, 'bootsnap');
  const dbDir = path.join(userDataPath, 'db');
  const logFile = path.join(userDataPath, 'ruby_sidecar.log');
  
  if (!fs.existsSync(dbDir)) fs.mkdirSync(dbDir, { recursive: true });

  if (fs.existsSync(pidFile)) {
    try {
      const oldPid = parseInt(fs.readFileSync(pidFile, 'utf8'));
      if (oldPid) {
        process.kill(oldPid, 'SIGTERM');
      }
    } catch (e) {}
    try { fs.unlinkSync(pidFile); } catch(e) {}
  }

  try { 
    if (process.platform !== 'win32') fs.chmodSync(rubyBinary, 0o755); 
  } catch(e) {}
  const pathSeparator = process.platform === 'win32' ? ';' : ':';

  const env = { 
    ...process.env, 
    PORT: '4567', 
    BUNDLE_GEMFILE: path.join(baseDir, 'Gemfile'),
    BUNDLE_DEPLOYMENT: 'true', 
    BUNDLE_PATH: path.join(resourceDir, 'vendor', 'bundle'),
    GEM_PATH: vendorGems,
    GEM_HOME: vendorGems,
    RUBY_PLATFORM_DIR: platformDir,
    RUBYLIB: "",
    BRILLIANT_DATA_DIR: userDataPath,
    BRILLIANT_ENV: 'electron',
    BOOTSNAP_CACHE_DIR: cacheDir,
    DATABASE_URL: `sqlite3:///${path.join(dbDir, 'production.sqlite3').replace(/\\/g, '/').replace(/ /g, '%20')}`,
    PATH: `${path.join(rubyBase, 'bin')}${pathSeparator}${process.env.PATH}`
  };

  console.log(`[Electron] Spawning Ruby: ${rubyBinary}`);
  
  let logFd;
  try {
    logFd = fs.openSync(logFile, 'a');
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
    console.error(`[Electron] Failed to start Ruby process: ${err}`);
    dialog.showErrorBox("Ruby Startup Error", `Failed to start the Ruby sidecar: ${err.message}\nBinary: ${rubyBinary}`);
  });

  rubyApp.on('exit', (code, signal) => {
    if (code !== 0 && code !== null) {
      console.error(`[Electron] Ruby process exited with code ${code}`);
      if (isPackaged) {
        dialog.showErrorBox("Ruby Sidecar Crash", `The Ruby backend process crashed with code ${code}.\nCheck the log at: ${logFile}`);
      }
    }
  });
}

app.on('ready', () => {
    startRubyApp();
    if (!process.argv.includes('--headless')) {
      createWindow();
    } else {
      console.log("[Electron] Running in headless mode. UI suppressed.");
    }
});

app.on('window-all-closed', function () {
  if (process.platform !== 'darwin') app.quit();
});

app.on('will-quit', () => {
  if (rubyApp) {
    rubyApp.kill();
  }
});

app.on('activate', function () {
  if (mainWindow === null) createWindow();
});
