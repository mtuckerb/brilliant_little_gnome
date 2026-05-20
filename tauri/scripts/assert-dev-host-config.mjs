import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const readText = (path) => readFileSync(resolve(root, path), 'utf8');
const readJson = (path) => JSON.parse(readText(path));

const config = readJson('src-tauri/tauri.conf.json');
const pkg = readJson('package.json');
const viteConfig = readText('vite.config.ts');
const iosDeployPath = 'scripts/ios-deploy.mjs';
const iosDeployScript = readText(iosDeployPath);

const devUrl = config?.build?.devUrl;
if (devUrl !== 'http://127.0.0.1:1420') {
  throw new Error('Expected build.devUrl to default to http://127.0.0.1:1420, got ' + devUrl);
}

for (const [name, script] of Object.entries(pkg.scripts ?? {})) {
  if (name === 'dev:ios:remote') continue;
  if (/169\.254\./.test(script)) {
    throw new Error('Script ' + name + ' must not use a link-local dev-server host by default: ' + script);
  }
}

const devIosScript = pkg.scripts?.['dev:ios'];
if (!devIosScript?.includes('node scripts/ios-deploy.mjs')) {
  throw new Error('Expected package.json script dev:ios to invoke node scripts/ios-deploy.mjs, got: ' + devIosScript);
}

if (!iosDeployScript.includes("spawnSync('npm', ['run', 'ios:build']")) {
  throw new Error('Expected ' + iosDeployPath + ' to build through npm run ios:build before deploying.');
}

const viteHostPinsLocalhost = /host:\s*remoteDevHost\s*\|\|\s*["']127\.0\.0\.1["']/.test(viteConfig);
if (!viteHostPinsLocalhost) {
  throw new Error('Expected vite.config.ts to pin the default iOS dev server host to 127.0.0.1 when BRILLIANT_TAURI_REMOTE_DEV is not enabled.');
}

if (!pkg.scripts?.['dev:ios:remote']?.includes('--force-ip-prompt')) {
  throw new Error('Expected npm run dev:ios:remote to explicitly prompt for the remote development host');
}

if (!viteConfig.includes('process.env.TAURI_DEV_HOST')) {
  throw new Error('Expected vite.config.ts to honor TAURI_DEV_HOST when Tauri provides a remote iOS dev host.');
}

console.log('iOS dev host config defaults to 127.0.0.1 and remote host mode is explicit.');
