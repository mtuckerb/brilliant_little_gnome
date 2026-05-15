#!/usr/bin/env node
// Build the iOS app and install it on a connected iPhone/iPad, bypassing
// `tauri ios dev`. Tauri 2's mobile CLI has an `xcodebuild -exportArchive
// -exportOptionsPlist <tempfile>` step that intermittently hands xcodebuild
// a temp path it can't open, exiting 70:
//
//     Error: Couldn't load -exportOptionsPlist The file "…" couldn't be opened
//
// `npm run ios:build` produces a valid `.ipa`; we extract the inner `.app`
// and hand it to `ios-deploy`, which installs and launches on the device.
//
// Usage:
//   node scripts/ios-deploy.mjs                  # build then install
//   node scripts/ios-deploy.mjs --no-build       # install most recent IPA
//   node scripts/ios-deploy.mjs --device <udid>  # target a specific device
//
// Requires:
//   - APPLE_DEVELOPMENT_TEAM env var (same as scripts/ios-build.mjs)
//   - `ios-deploy` on PATH (in shell.nix)
//   - device connected via USB and trusted

import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const projectDir = join(scriptDir, '..');

const args = process.argv.slice(2);
let doBuild = true;
let deviceId = null;
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === '--no-build') doBuild = false;
  else if (a === '--device') deviceId = args[++i];
  else {
    console.error(`Unknown argument: ${a}`);
    process.exit(2);
  }
}

if (doBuild) {
  console.log('==> Building iOS app (development)…');
  const build = spawnSync('npm', ['run', 'ios:build'], {
    cwd: projectDir,
    stdio: 'inherit',
  });
  if (build.status !== 0) {
    console.error('iOS build failed.');
    process.exit(build.status ?? 1);
  }
}

const ipa = join(projectDir, 'src-tauri/gen/apple/build/arm64/Brilliant.ipa');
if (!existsSync(ipa)) {
  console.error(`IPA not found at ${ipa}. Run without --no-build, or check ios:build output.`);
  process.exit(1);
}

const scratch = mkdtempSync(join(tmpdir(), 'brilliant-ipa-'));
const cleanup = () => { try { rmSync(scratch, { recursive: true, force: true }); } catch {} };
process.on('exit', cleanup);
process.on('SIGINT', () => { cleanup(); process.exit(130); });

console.log(`==> Extracting ${ipa}…`);
const unzip = spawnSync('unzip', ['-q', ipa, '-d', scratch], { stdio: 'inherit' });
if (unzip.status !== 0) {
  console.error('Failed to unzip IPA.');
  process.exit(unzip.status ?? 1);
}

const payloadDir = join(scratch, 'Payload');
const appName = readdirSync(payloadDir).find((n) => n.endsWith('.app'));
if (!appName) {
  console.error(`No .app bundle inside ${ipa}`);
  process.exit(1);
}
const appPath = join(payloadDir, appName);
console.log(`==> Found ${appName}`);

const deployArgs = ['--bundle', appPath, '--justlaunch', '--noninteractive'];
if (deviceId) deployArgs.push('--id', deviceId);

console.log('==> Installing on device…');
const deploy = spawnSync('ios-deploy', deployArgs, { stdio: 'inherit' });
if (deploy.error && deploy.error.code === 'ENOENT') {
  console.error('ios-deploy not found on PATH. It is in shell.nix — run inside the dev shell.');
  process.exit(1);
}
process.exit(deploy.status ?? 1);
