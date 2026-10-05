#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

// Run Tao's actual iOS state-machine regressions, rather than a desktop mock.
// Requires the aarch64-apple-ios-sim target and a booted iOS simulator.
const cwd = fileURLToPath(new URL('../src-tauri/', import.meta.url));
const build = spawnSync('cargo', [
  'test', '--manifest-path', 'vendor/tao/Cargo.toml', '--lib',
  '--target', 'aarch64-apple-ios-sim', '--no-default-features',
  '--features', 'rwh_06', '--no-run', '--message-format=json',
], {
  cwd,
  env: { ...process.env, CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? join(tmpdir(), 'brilliant-tao-ios-tests') },
  encoding: 'utf8',
  stdio: ['ignore', 'pipe', 'inherit'],
  maxBuffer: 16 * 1024 * 1024,
});
if (build.error) throw build.error;
if (build.status !== 0) process.exit(build.status ?? 1);
const binary = build.stdout.split('\n').filter(Boolean).map(line => JSON.parse(line))
  .find(message => message.reason === 'compiler-artifact' && message.target.name === 'tao' && message.executable)
  ?.executable;
if (!binary) throw new Error('Cargo did not return the iOS lifecycle test executable');
const run = spawnSync('xcrun', [
  'simctl', 'spawn', process.env.BRILLIANT_TEST_SIMULATOR ?? 'booted', binary,
  'brilliant_lifecycle_tests', '--test-threads=1', '--nocapture',
], { stdio: 'inherit' });
if (run.error) throw run.error;
process.exit(run.status ?? 1);
