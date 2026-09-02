# iOS builds

Brilliant's Tauri iOS release build must be signed and exported with an explicit Xcode export method. The helper scripts in `package.json` keep the export method explicit and inject the Apple Team ID from the environment so the repository does not commit account-specific signing data.

## Prerequisites

- Xcode with iOS platform support installed.
- A connected iPad trusted by the Mac, or an Apple Developer account that can create the required provisioning profile.
- `APPLE_DEVELOPMENT_TEAM` set to your 10-character Apple Developer Team ID. Find it on the Apple Developer Membership page, then put it in your local shell or an untracked `.env` file.

```sh
export APPLE_DEVELOPMENT_TEAM=YOURTEAMID
```

Do not commit your real Team ID. `tauri/src-tauri/tauri.conf.json` only keeps portable iOS defaults; the build helper passes the Team ID via Tauri's `--config` override.

## Build commands

For installing on a personal development device:

```sh
npm run ios:build:dev
```

For ad-hoc distribution:

```sh
npm run ios:build:adhoc
```

For App Store / TestFlight export:

```sh
npm run ios:build:app-store
```

Each script runs `tauri ios build --export-method ...` so Tauri does not fall back to a transient default export plist path during `xcodebuild -exportArchive`.

## ExportOptions.plist

`src-tauri/gen/apple/ExportOptions.plist` is checked in as a stable fallback for Xcode export settings. The build scripts still pass `--export-method` explicitly; the checked-in plist avoids relying solely on Tauri's temporary mktemp file behavior.

## Installing the IPA

- Development/ad-hoc: open Xcode -> Window -> Devices and Simulators, select the iPad, and drag the exported `.ipa` onto the installed apps list.
- TestFlight: run `scripts/release-testflight.sh` (see below), or upload the App Store export by hand through App Store Connect.

## TestFlight releases

`scripts/release-testflight.sh` is the one-command local release path (the iOS
counterpart to `release-desktop.sh` — no CI involved): it builds the
`app-store-connect` export, validates the IPA against App Store Connect, and
uploads it to TestFlight with `xcrun altool`.

On top of the build prerequisites above it needs App Store Connect
credentials, either of:

- **API key** (preferred): set `APP_STORE_CONNECT_KEY_ID` and
  `APP_STORE_CONNECT_ISSUER_ID`, and put the matching `AuthKey_<KEY_ID>.p8`
  in `~/.appstoreconnect/private_keys/`. Keys are minted at App Store
  Connect -> Users and Access -> Integrations.
- **Apple ID fallback**: set `APPLE_ID` and `APPLE_APP_PASSWORD` (an
  app-specific password from appleid.apple.com — your normal password won't
  work).

```sh
export APPLE_DEVELOPMENT_TEAM=YOURTEAMID
scripts/release-testflight.sh
```

The uploaded version is whatever `src-tauri/tauri.conf.json` says. App Store
Connect rejects a version+build it has already seen for iOS, so bump the
version there (the usual `chore(release)` commit) before re-releasing.

The release IPA should launch from bundled assets and should not require the Vite dev server. Use `npm run dev:ios` only for the live-development workflow.
