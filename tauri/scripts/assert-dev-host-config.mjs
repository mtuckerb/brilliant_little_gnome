import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const readJson = (path) => JSON.parse(readFileSync(resolve(root, path), 'utf8'));

const config = readJson('src-tauri/tauri.conf.json');
const pkg = readJson('package.json');
const viteConfig = readFileSync(resolve(root, 'vite.config.ts'), 'utf8');

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

if (!pkg.scripts?.['dev:ios']?.includes('--host 127.0.0.1')) {
  throw new Error('Expected npm run dev:ios to pin Tauri iOS dev to --host 127.0.0.1');
}

if (!pkg.scripts?.['dev:ios:remote']?.includes('BRILLIANT_TAURI_REMOTE_DEV=1')) {
  throw new Error('Expected npm run dev:ios:remote to explicitly opt into remote development host handling');
}

if (!viteConfig.includes('BRILLIANT_TAURI_REMOTE_DEV')) {
  throw new Error('Expected vite.config.ts to gate TAURI_DEV_HOST behind BRILLIANT_TAURI_REMOTE_DEV');
}

console.log('iOS dev host config defaults to 127.0.0.1 and remote host mode is explicit.');
