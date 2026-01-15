const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

const baseDir = __dirname;
const platformDir = 'macos-arm64';
const rubyExec = 'ruby';

const rubyBase = path.join(baseDir, 'bin', 'ruby_dist', platformDir);
const rubyBinary = path.join(rubyBase, 'bin', rubyExec);
const bundleBinary = path.join(rubyBase, 'bin', 'bundle');

const vendorGems = path.join(baseDir, 'vendor', 'bundle', 'ruby', '3.4.0');
const internalGems = path.join(rubyBase, 'lib', 'ruby', 'gems', '3.4.0');

// Simulation of Electron's app.getPath('userData')
const userDataPath = path.join(os.homedir(), 'Library', 'Application Support', 'Brilliant-Test');
const cacheDir = path.join(userDataPath, 'bootsnap');
const dbDir = path.join(userDataPath, 'db');

const env = { 
  ...process.env, 
  PORT: '4567', 
  BUNDLE_GEMFILE: path.join(baseDir, 'Gemfile'),
  BUNDLE_DEPLOYMENT: 'true', 
  BUNDLE_PATH: path.join(baseDir, 'vendor', 'bundle'),
  GEM_PATH: `${vendorGems}:${internalGems}`,
  GEM_HOME: vendorGems,
  RUBY_PLATFORM_DIR: platformDir,
  BRILLIANT_DATA_DIR: userDataPath,
  BRILLIANT_ENV: 'electron',
  BOOTSNAP_CACHE_DIR: cacheDir,
  DATABASE_URL: `sqlite3:///${path.join(dbDir, 'production.sqlite3').replace(/ /g, '%20')}`,
  PATH: `${path.join(rubyBase, 'bin')}:${process.env.PATH}`
};

delete env.RUBYLIB;
delete env.RUBYOPT;

console.log("Starting Ruby Debug Session...");
console.log(`Working Dir: ${baseDir}`);
console.log(`Database: ${env.DATABASE_URL}`);

const rubyApp = spawn(rubyBinary, [bundleBinary, 'exec', 'ruby', 'app.rb'], {
  cwd: baseDir,
  env: env
});

rubyApp.stdout.on('data', (data) => {
  console.log(`STDOUT: ${data}`);
});

rubyApp.stderr.on('data', (data) => {
  console.error(`STDERR: ${data}`);
});

rubyApp.on('exit', (code) => {
  console.log(`Ruby process exited with code ${code}`);
});

process.on('SIGINT', () => {
  rubyApp.kill();
  process.exit();
});
