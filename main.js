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
      preload: path.join(__dirname, 'preload.js'),
      spellcheck: true
    }
  });

  // Set a standard browser User Agent to help with 1Password/Password Manager detection
  const standardUA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
  mainWindow.webContents.setUserAgent(standardUA);

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

ipcMain.on('start-login', async (event, host) => {
  // Clear ALL cookies from Brightspace and common SSO providers to ensure a clean login
  // This prevents stale sessions from causing "Invalid Username/Password" errors
  const domainsToClear = [host];
  if (host.includes('.')) {
    const parentDomain = host.substring(host.indexOf('.'));
    domainsToClear.push(parentDomain);
  }
  // Also clear common SSO provider domains that might have stale auth state
  const ssoDomains = ['idp.maine.edu', 'accounts.google.com', 'login.microsoftonline.com'];
  domainsToClear.push(...ssoDomains);
  
  try {
    let clearedCount = 0;
    for (const domain of domainsToClear) {
      try {
        const cookies = await session.defaultSession.cookies.get({ domain: domain });
        for (const cookie of cookies) {
          const protocol = cookie.secure ? 'https' : 'http';
          const cookieDomain = cookie.domain.startsWith('.') ? cookie.domain.substring(1) : cookie.domain;
          const url = `${protocol}://${cookieDomain}${cookie.path}`;
          await session.defaultSession.cookies.remove(url, cookie.name);
          clearedCount++;
        }
      } catch (domainErr) {
        // Some domains may not have cookies, that's fine
      }
    }
    if (clearedCount > 0) {
      console.log(`[Electron] Cleared ${clearedCount} cookies from ${domainsToClear.length} domains`);
    }
  } catch (err) {
    console.error(`[Electron] Error clearing cookies: ${err.message}`);
  }

  const loginWindow = new BrowserWindow({
    width: 800,
    height: 900,
    show: false, // Don't show until it starts loading to avoid white window
    title: "Brilliant | Sign In to " + host,
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      spellcheck: true
    }
  });

  loginWindow.center();

  // Set a standard browser User Agent to help with 1Password/Password Manager detection
  const standardUA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
  loginWindow.webContents.setUserAgent(standardUA);

  // Clear storage only on the initial Brightspace page load, not on SSO pages
  // (SSO providers like Google/Azure AD use sessionStorage for CSRF state)
  let storageCleared = false;
  loginWindow.webContents.on('dom-ready', () => {
    if (!storageCleared) {
      storageCleared = true;
      loginWindow.webContents.executeJavaScript(`
        try { localStorage.clear(); } catch(e) {}
        try { sessionStorage.clear(); } catch(e) {}
      `).catch(() => {});
    }
  });

  // Navigate to /d2l/home which redirects through SSO/Google OAuth naturally.
  // Avoid login.d2l — it fires "Invalid Username/Password" alerts on stale/cleared sessions.
  loginWindow.loadURL(`https://${host}/d2l/home`).catch(err => {
    console.error(`[Electron] Failed to load login URL: ${err.message}`);
    loginWindow.loadURL(`https://${host}/d2l/login`).catch(err2 => {
      console.error(`[Electron] Fallback URL also failed: ${err2.message}`);
    });
  });

  loginWindow.once('ready-to-show', () => {
    loginWindow.show();
  });

  loginWindow.webContents.on('did-fail-load', (event, errorCode, errorDescription) => {
    console.error(`[Electron] Login window failed to load: ${errorDescription} (${errorCode})`);
    // -3 = aborted (normal during OAuth redirects), -102 = connection refused
    if (errorCode !== -3 && errorCode !== -102) {
       loginWindow.loadURL(`https://${host}/d2l/login`).catch(err => {
         console.error(`[Electron] Fallback URL also failed: ${err.message}`);
       });
    }
  });

  loginWindow.webContents.on('did-navigate', (event, url) => {
    checkLoginSuccess(url);
  });

  loginWindow.webContents.on('did-frame-navigate', (event, url) => {
    checkLoginSuccess(url);
  });

  async function checkLoginSuccess(url) {
    // Check for the secure session cookie across host and parent domain
    // (cookies may be set on either scope)
    const domainsToCheck = [host];
    if (host.includes('.')) {
      const parentDomain = host.substring(host.indexOf('.'));
      domainsToCheck.push(parentDomain);
    }
    
    let allCookies = [];
    for (const domain of domainsToCheck) {
      const domainCookies = await session.defaultSession.cookies.get({ domain: domain });
      allCookies = allCookies.concat(domainCookies);
    }
    
    const uniqueCookies = allCookies.filter((cookie, index, self) => 
      index === self.findIndex(c => c.name === cookie.name && c.domain === cookie.domain)
    );
    
    const hasSession = uniqueCookies.some(c => c.name === 'd2lSecureSessionVal');

    // Only consider login complete when we have the session cookie.
    // URL check alone is insufficient — /d2l/home is our initial load URL,
    // and Brightspace will redirect away to SSO if not authenticated.
    // We need the cookie to confirm auth actually completed.
    if (hasSession) {
      console.log(`[Electron] Login successful (cookie found) on ${url}. Capturing ${uniqueCookies.length} unique cookies.`);
      const cookieString = uniqueCookies.map(c => `${c.name}=${c.value}`).join('; ');
      mainWindow.webContents.send('login-complete', { host, cookies: cookieString });
      
      setTimeout(() => {
        loginWindow.close();
      }, 1000);
    }
  }

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

  const portableRubyBase = path.join(resourceDir, 'bin', 'ruby_dist', platformDir);
  let rubyBinary = path.join(portableRubyBase, 'bin', rubyExec);

  let usePortable = fs.existsSync(rubyBinary);

  // Fallback to system Ruby if portable distribution is missing
  if (!usePortable) {
    if (isPackaged) {
      console.warn(`[Electron] Portable Ruby not found at ${rubyBinary}. Falling back to system Ruby (Risky!).`);
    } else {
      console.log(`[Electron] Portable Ruby not found. Using system Ruby.`);
    }
    rubyBinary = rubyExec;
  }

  // Detect Ruby version directory in vendor/bundle/ruby/
  // Must match the portable Ruby's actual version (e.g., 3.4.0), not stale leftovers
  let rubyVersionDir = usePortable ? '3.4.0' : '3.1.0'; // Best guess fallbacks
  const vendorRubyRoot = path.join(resourceDir, 'vendor', 'bundle', 'ruby');
  if (fs.existsSync(vendorRubyRoot)) {
    const versions = fs.readdirSync(vendorRubyRoot)
      .filter(f => fs.statSync(path.join(vendorRubyRoot, f)).isDirectory())
      .sort((a, b) => b.localeCompare(a, undefined, { numeric: true })); // Descending: prefer latest
    if (versions.length > 0) {
      // If using portable Ruby, match against its actual gems directory
      if (usePortable) {
        const portableGemsRoot = path.join(portableRubyBase, 'lib', 'ruby', 'gems');
        if (fs.existsSync(portableGemsRoot)) {
          const portableVersions = fs.readdirSync(portableGemsRoot).filter(f => fs.statSync(path.join(portableGemsRoot, f)).isDirectory());
          const match = versions.find(v => portableVersions.includes(v));
          if (match) {
            rubyVersionDir = match;
          } else {
            rubyVersionDir = versions[0]; // Fall back to latest vendor version
          }
        } else {
          rubyVersionDir = versions[0];
        }
      } else {
        rubyVersionDir = versions[0];
      }
      console.log(`[Electron] Detected Ruby version directory: ${rubyVersionDir}`);
    }
  }

  const vendorGems = path.join(resourceDir, 'vendor', 'bundle', 'ruby', rubyVersionDir);
  const internalGems = path.join(portableRubyBase, 'lib', 'ruby', 'gems', rubyVersionDir);

  // Find Bundler path and Ruby stdlib path for RUBYLIB
  let rubyLibPaths = [];
  if (usePortable) {
    // Add Ruby stdlib path so packaged builds can find rubygems.rb even if
    // the binary's compiled-in paths don't match the actual install location
    const stdlibPath = path.join(portableRubyBase, 'lib', 'ruby', rubyVersionDir);
    if (fs.existsSync(stdlibPath)) {
      rubyLibPaths.push(stdlibPath);
      // Also add the architecture-specific subdirectory
      const archDirs = fs.readdirSync(stdlibPath).filter(f => {
        const fullPath = path.join(stdlibPath, f);
        return fs.statSync(fullPath).isDirectory() && (f.includes('darwin') || f.includes('mingw'));
      });
      for (const archDir of archDirs) {
        rubyLibPaths.push(path.join(stdlibPath, archDir));
      }
    }

    // Add site_ruby paths
    const siteRubyPath = path.join(portableRubyBase, 'lib', 'ruby', 'site_ruby', rubyVersionDir);
    if (fs.existsSync(siteRubyPath)) {
      rubyLibPaths.push(siteRubyPath);
    }

    const internalGemsGems = path.join(internalGems, 'gems');
    if (fs.existsSync(internalGemsGems)) {
      const dirs = fs.readdirSync(internalGemsGems);
      const bundlerDir = dirs.find(d => d.startsWith('bundler-'));
      if (bundlerDir) {
        rubyLibPaths.push(path.join(internalGemsGems, bundlerDir, 'lib'));
      }
    }
  }

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
    if (process.platform !== 'win32' && fs.existsSync(rubyBinary)) fs.chmodSync(rubyBinary, 0o755); 
  } catch(e) {}
  const pathSeparator = process.platform === 'win32' ? ';' : ':';

  const env = {
    ...process.env, 
    PORT: '4567', 
    BUNDLE_GEMFILE: path.join(baseDir, 'Gemfile'),
    BUNDLE_DEPLOYMENT: 'true', 
    BUNDLE_PATH: path.join(resourceDir, 'vendor', 'bundle'),
    GEM_PATH: usePortable ? `${vendorGems}${pathSeparator}${internalGems}` : vendorGems,
    GEM_HOME: vendorGems,
    RUBYLIB: rubyLibPaths.join(pathSeparator),
    BRILLIANT_DATA_DIR: userDataPath,
    BRILLIANT_ENV: 'electron',
    BOOTSNAP_CACHE_DIR: cacheDir,
    DATABASE_URL: `sqlite3:///${path.join(dbDir, 'production.sqlite3').replace(/\\/g, '/').replace(/ /g, '%20')}`,
    PATH: usePortable ? `${path.join(portableRubyBase, 'bin')}${pathSeparator}${process.env.PATH}` : process.env.PATH
  };

  // Add DYLD_LIBRARY_PATH if using portable Ruby on macOS to assist with library resolution
  // (though SIP usually blocks this for system binaries, it works for our portable ones)
  if (process.platform === 'darwin' && usePortable) {
    const rubyLibDir = path.join(portableRubyBase, 'lib');
    if (fs.existsSync(rubyLibDir)) {
       env.DYLD_LIBRARY_PATH = `${rubyLibDir}${pathSeparator}${process.env.DYLD_LIBRARY_PATH || ''}`;
    }
  }

  console.log(`[Electron] Spawning Ruby: ${rubyBinary}`);
  
  let logFd;
  try {
    logFd = fs.openSync(logFile, 'a');
  } catch (err) {
    console.error("Failed to open log file:", err);
  }

  // Do not forward Electron/Chromium CLI flags into Sinatra.
  // Flags like "--port" can be interpreted by Sinatra and crash boot.
  const rubyArgs = ['app.rb'];

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
