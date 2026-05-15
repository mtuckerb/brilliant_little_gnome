#!/usr/bin/env node
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const projectDir = join(scriptDir, '..');
const nodeModulesDir = join(projectDir, 'node_modules');

if (existsSync(nodeModulesDir)) {
  process.exit(0);
}

console.log('tauri/node_modules is missing; installing frontend dependencies before build...');

const install = spawnSync('npm', ['install', '--no-audit', '--no-fund'], {
  cwd: projectDir,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

if (install.status !== 0) {
  process.exit(install.status ?? 1);
}
