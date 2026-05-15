# iOS and iPadOS builds

Brilliant has two iOS/iPadOS workflows. They intentionally behave differently.

## Development workflow: Vite dev server

Use this only while actively developing from a Mac:

```sh
cd tauri
npm install
npm run dev:ios
```

`tauri ios dev` starts Vite from `beforeDevCommand` and loads the WebView from the configured `devUrl` (`http://127.0.0.1:1420`, or your prompted LAN address when using the remote dev script). This is normal for development builds because it enables hot module reload.

Implications:

- The iPad may request local-network permission.
- The Mac must stay awake and reachable on the same network.
- The app will stop loading its React UI if the Vite dev server is unavailable.
- Dev builds show a small `Dev server: host:port` badge in the navbar.

Remote-device development remains available when the iPad cannot reach `127.0.0.1`:

```sh
cd tauri
npm run dev:ios:remote
```

## Standalone release workflow: embedded frontend

Use this for an iPad/iPhone install that must work without the Mac or Vite:

```sh
cd tauri
npm install
npm run ios:build
```

`npm run ios:build` runs `tauri ios build`, which uses `beforeBuildCommand` (`npm run build`) and embeds the generated `dist/` assets through `frontendDist: "../dist"`. The installed app loads the React shell from its app bundle and should never contact the Vite dev server on launch.

To open the generated Xcode project for signing, archiving, or installing a release-style build on a device:

```sh
cd tauri
npm run ios:open
```

In Xcode, use a Release scheme/archive when validating standalone behavior. A release install can be launched with the Mac offline or the Vite dev server stopped; first launch should still show the Brilliant setup/auth shell, and later launches should show cached local SQLite data from the app container.

## Safety checks

Release builds include a startup guard: if the main WebView is ever configured with an `http://` or `https://` origin, the app refuses to start and logs a clear error. This prevents a misconfigured release from silently falling back to the dev server.

## Sync note

Standalone iOS builds are independent of the Vite dev server. Device-to-device sync is handled by Brilliant's paired-device sync feature when enabled; it is separate from the dev-server workflow and does not require the Mac's Vite process.
