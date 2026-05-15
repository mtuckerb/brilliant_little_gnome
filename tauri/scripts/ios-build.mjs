#!/usr/bin/env node
import { spawnSync } from 'node:child_process';

const methodAliases = new Map([
  ['dev', 'development'],
  ['development', 'development'],
  ['adhoc', 'ad-hoc'],
  ['ad-hoc', 'ad-hoc'],
  ['app-store', 'app-store'],
  ['appstore', 'app-store'],
]);

const requestedMethod = process.argv[2] ?? 'development';
const exportMethod = methodAliases.get(requestedMethod);

if (!exportMethod) {
  console.error(`Unsupported iOS export method: ${requestedMethod}`);
  console.error('Expected one of: development, ad-hoc, app-store');
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
