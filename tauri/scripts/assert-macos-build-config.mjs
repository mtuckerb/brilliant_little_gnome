import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(root, '..');
const readJson = (path) => JSON.parse(readFileSync(resolve(root, path), 'utf8'));
const readRepo = (path) => readFileSync(resolve(repoRoot, path), 'utf8');

const pkg = readJson('package.json');
const config = readJson('src-tauri/tauri.conf.json');
const releaseWorkflow = readRepo('.github/workflows/release.yml');
const tauriReadme = readRepo('tauri/README.md');
const rootReadme = readRepo('README.md');

const requireScript = (name, expected) => {
  const actual = pkg.scripts?.[name];
  if (actual !== expected) {
    throw new Error(`Expected npm script ${name} to be ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
};

requireScript('macos:build', 'npm run ensure:deps && tauri build --target universal-apple-darwin');
requireScript('macos:build:local', 'npm run macos:build');
requireScript('macos:dev', 'npm run ensure:deps && tauri dev');

if (config?.bundle?.targets !== 'all') {
  throw new Error('Expected Tauri bundle.targets to remain "all" so macOS app/dmg bundles are produced.');
}

if (!config?.bundle?.macOS || config.bundle.macOS.minimumSystemVersion !== '11.0') {
  throw new Error('Expected bundle.macOS.minimumSystemVersion to document the supported macOS baseline.');
}

if (!releaseWorkflow.includes('--target universal-apple-darwin')) {
  throw new Error('Expected release workflow to build universal macOS artifacts.');
}

if (!tauriReadme.includes('npm run macos:build')) {
  throw new Error('Expected tauri/README.md to document npm run macos:build.');
}

if (!tauriReadme.includes('Transporter')) {
  throw new Error('Expected tauri/README.md to identify the macOS Transporter/Brilliant build.');
}

if (!rootReadme.includes('npm run macos:build')) {
  throw new Error('Expected root README.md to document the local macOS build command.');
}

console.log('macOS Transporter build configuration is documented and release-ready.');
