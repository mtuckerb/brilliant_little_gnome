#!/usr/bin/env node
import { spawnSync } from 'node:child_process';

// Maps the user-facing alias we accept on the CLI to the value tauri-cli
// expects via `--export-method`. Recent tauri-cli renamed these:
//   "development"  →  "debugging"
//   "ad-hoc"       →  "release-testing"
//   "app-store"    →  "app-store-connect"
// Keeping the old aliases so existing `npm run ios:build:*` scripts and
// muscle memory continue to work.
const methodAliases = new Map([
  ['dev', 'debugging'],
  ['development', 'debugging'],
  ['debug', 'debugging'],
  ['debugging', 'debugging'],
  ['adhoc', 'release-testing'],
  ['ad-hoc', 'release-testing'],
  ['release-testing', 'release-testing'],
  ['app-store', 'app-store-connect'],
  ['appstore', 'app-store-connect'],
  ['app-store-connect', 'app-store-connect'],
]);

const requestedMethod = process.argv[2] ?? 'development';
const exportMethod = methodAliases.get(requestedMethod);

if (!exportMethod) {
  console.error(`Unsupported iOS export method: ${requestedMethod}`);
  console.error('Expected one of: debugging (alias: development), release-testing (alias: ad-hoc), app-store-connect (alias: app-store)');
  process.exit(2);
}

const developmentTeam = process.env.APPLE_DEVELOPMENT_TEAM;

if (!developmentTeam) {
  console.error('APPLE_DEVELOPMENT_TEAM is required for iOS archive export.');
  console.error('Set it to your Apple Developer Team ID in a local .env or shell environment.');
  process.exit(2);
}

if (!/^[A-Z0-9]{10}$/.test(developmentTeam)) {
  console.error('APPLE_DEVELOPMENT_TEAM must look like a 10-character Apple Team ID.');
  process.exit(2);
}

const config = JSON.stringify({
  bundle: {
    iOS: {
      developmentTeam,
      minimumSystemVersion: '13.0',
    },
  },
});

const result = spawnSync(
  'npm',
  [
    'run',
    'tauri',
    '--',
    'ios',
    'build',
    '--export-method',
    exportMethod,
    '--config',
    config,
  ],
  { stdio: 'inherit' },
);

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
