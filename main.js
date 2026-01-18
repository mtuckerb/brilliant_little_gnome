const { app, BrowserWindow, ipcMain, session } = require('electron');
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
    if (url.includes('/download')) {
      return { action: 'allow' };
    }
    return { action: 'deny' };
  });

  mainWindow.webContents.on('will-navigate', (event, url) => {
    if (url.includes('/download')) {
      // Allow the navigation but handle as download
    }
  });

  session.defaultSession.on('will-download', (event, item, webContents) => {
    // Set the save path, which making Electron show the save dialog
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
    title: "Brightspace Login",
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true
    }
  });

  loginWindow.loadURL(`https://${host}/d2l/lp/auth/login/login.d2l`);

  // Suppress Brightspace's annoying alerts - injected at start and when DOM is ready
  loginWindow.webContents.on('dom-ready', () => {
    const script = "window.alert = function(){}; window.confirm = function(){return true;}; window.prompt = function(){return null;};";
    loginWindow.webContents.executeJavaScript(script);
  });

  loginWindow.webContents.on('did-navigate', (event, url) => {
    if (url.includes("/d2l/home") || url.includes("/d2l/lp/homepage")) {
      // Extract cookies
      session.defaultSession.cookies.get({ domain: host })
        .then((cookies) => {
          const cookieString = cookies.map(c => `${c.name}=${c.value}`).join('; ');
          
          // Send back to Sinatra via main window (or we can use an IPC -> Ruby bridge)
          // For now, let's send it back to the renderer to POST it to Sinatra
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
    // Handle cancellation
  });
});

function startRubyApp() {
  const isPackaged = app.isPackaged;
  const baseDir = isPackaged ? app.getAppPath().replace('app.asar', 'app.asar.unpacked') : __dirname;
  const userDataPath = app.getPath('userData');
  const pidFile = path.join(userDataPath, 'ruby_sidecar.pid');

  // --- PID FAILSAFE ---
  if (fs.existsSync(pidFile)) {
    try {
      const oldPid = parseInt(fs.readFileSync(pidFile, 'utf8'));
      if (oldPid) {
        process.kill(oldPid, 'SIGTERM');
        console.log(`Killed stale Ruby process: ${oldPid}`);
      }
    } catch (e) {
      console.log("Stale PID file found but process already dead.");
    }
    fs.unlinkSync(pidFile);
  }
  // --------------------

  let platformDir = '';
  let rubyExec = 'ruby';

  if (process.platform === 'darwin') {
    const arch = process.arch === 'arm64' ? 'arm64' : 'x64';
    platformDir = `macos-${arch}`;
  } else if (process.platform === 'win32') {
    platformDir = 'win-x64';
    rubyExec = 'ruby.exe';
  }

  const rubyBase = path.join(baseDir, 'bin', 'ruby_dist', platformDir);
  const rubyBinary = path.join(rubyBase, 'bin', rubyExec);
  const bundleBinary = path.join(rubyBase, 'bin', 'bundle');

  const vendorGems = path.join(baseDir, 'vendor', 'bundle', 'ruby', '3.4.0');
  const internalGems = path.join(rubyBase, 'lib', 'ruby', 'gems', '3.4.0');
  
  const cacheDir = path.join(userDataPath, 'bootsnap');
  const dbDir = path.join(userDataPath, 'db');
  const logFile = path.join(userDataPath, 'ruby_sidecar.log');
  
  if (!fs.existsSync(dbDir)) fs.mkdirSync(dbDir, { recursive: true });
  try { fs.chmodSync(rubyBinary, 0o755); } catch(e) {}

  const pathSeparator = process.platform === 'win32' ? ';' : ':';
  const env = { 
    ...process.env, 
    PORT: '4567', 
    BUNDLE_GEMFILE: path.join(baseDir, 'Gemfile'),
    BUNDLE_DEPLOYMENT: 'true', 
    BUNDLE_PATH: path.join(baseDir, 'vendor', 'bundle'),
    GEM_PATH: `${vendorGems}${pathSeparator}${internalGems}`,
    GEM_HOME: vendorGems,
    RUBY_PLATFORM_DIR: platformDir,
    BRILLIANT_DATA_DIR: userDataPath,
    BRILLIANT_ENV: 'electron',
    BOOTSNAP_CACHE_DIR: cacheDir,
    DATABASE_URL: `sqlite3:///${path.join(dbDir, 'production.sqlite3').replace(/\\/g, '/').replace(/ /g, '%20')}`,
    PATH: `${path.join(rubyBase, 'bin')}${pathSeparator}${process.env.PATH}`
  };

  // Fix: Use absolute path to bundle binary and ensure we use the vendored ruby
  // We'll also log the spawn command for debugging
  console.log(`[Electron] Spawning Ruby: ${rubyBinary} ${bundleBinary} exec ruby app.rb`);
  console.log(`[Electron] Working Directory: ${baseDir}`);

  rubyApp = spawn(rubyBinary, [bundleBinary, 'exec', 'ruby', 'app.rb'], {
    cwd: baseDir,
    env: env
  });

  rubyApp.on('error', (err) => {
    console.error(`[Electron] Failed to start Ruby process: ${err}`);
  });

  const logStream = fs.createWriteStream(logFile, { flags: 'a' });
  logStream.write(`\n--- Started at ${new Date().toISOString()} ---\n`);

  rubyApp.stdout.on('data', (data) => {
    logStream.write(`STDOUT: ${data}`);
  });

  rubyApp.stderr.on('data', (data) => {
    logStream.write(`STDERR: ${data}`);
  });

  rubyApp.on('exit', (code) => {
    logStream.write(`EXIT: Ruby exited with code ${code}\n`);
  });
}

app.on('ready', () => {
    startRubyApp();
    createWindow();
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
