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
- TestFlight: upload the App Store export through the normal App Store Connect/TestFlight flow.

The release IPA should launch from bundled assets and should not require the Vite dev server. Use `npm run dev:ios` only for the live-development workflow.
