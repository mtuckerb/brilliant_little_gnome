# Dev shell for the Tauri port. Usage: `nix-shell` from this directory or
# `nix develop` from the repository root. Cross-platform: NixOS (Linux) +
# nix-darwin (macOS). Pulls a fixed recent stable Rust via oxalica/rust-overlay
# so both machines agree on the toolchain regardless of which channel the host
# nixpkgs is on.

{ system ? builtins.currentSystem
, nixpkgsSrc ? fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/nixos-24.05.tar.gz";
  }
, rustOverlaySrc ? fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/master.tar.gz";
  }
, unstableNixpkgsSrc ? fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/nixos-unstable.tar.gz";
  }
, rustOverlay ? import rustOverlaySrc
, pkgs ? import nixpkgsSrc { inherit system; overlays = [ rustOverlay ]; }
, pkgsUnstable ? import unstableNixpkgsSrc { inherit system; }
}:

let
  inherit (pkgs) lib stdenv;

  # Mobile cross-compile targets. iOS targets only make sense on Darwin (need
  # Xcode at build time); Android targets are added on both hosts since the
  # NDK can be installed separately on either. rust-overlay just downloads
  # the precompiled stdlib for these triples — actual builds still require
  # the platform SDK to be present.
  iosTargets = [ "aarch64-apple-ios" "aarch64-apple-ios-sim" ];
  androidTargets = [
    "aarch64-linux-android"
    "armv7-linux-androideabi"
    "i686-linux-android"
    "x86_64-linux-android"
  ];

  # Single source of truth for the toolchain. Bump as needed.
  rustToolchain = pkgs.rust-bin.stable."1.90.0".default.override {
    extensions = [ "rust-src" "rustfmt" "clippy" "rust-analyzer" ];
    targets = androidTargets ++ lib.optionals stdenv.isDarwin iosTargets;
  };

  # Libraries with pkg-config metadata needed by cargo builds/tests. Keep this
  # list separate from buildInputs so shellHook can publish deterministic
  # PKG_CONFIG_PATH values even in non-interactive `nix-shell --run ...` and
  # `nix develop --command ...` checks.
  pkgConfigDeps = with pkgs; [
    openssl
  ] ++ lib.optionals stdenv.isLinux [
    webkitgtk_4_1
    gtk3
    cairo
    gdk-pixbuf
    glib
    pango
    harfbuzz
    libsoup_3
    librsvg
    atk
  ];
in
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  buildInputs = with pkgs; [
    rustToolchain

    # Node + build tools (cross-platform)
    nodejs_20
    openssl
  ] ++ lib.optionals stdenv.isLinux [
    # Tauri Linux runtime deps (webkit2gtk + the GLib/GTK stack)
    webkitgtk_4_1
    gtk3
    cairo
    gdk-pixbuf
    glib
    pango
    harfbuzz
    libsoup_3
    librsvg
    atk
    gcc
  ] ++ lib.optionals stdenv.isDarwin [
    libiconv
    darwin.apple_sdk.frameworks.WebKit
    darwin.apple_sdk.frameworks.AppKit
    darwin.apple_sdk.frameworks.Cocoa
    darwin.apple_sdk.frameworks.Security
    darwin.apple_sdk.frameworks.CoreServices
    darwin.apple_sdk.frameworks.CoreFoundation
    darwin.apple_sdk.frameworks.Carbon

    # iOS mobile build helpers used by `tauri ios init/dev/build`. Tauri's
    # mobile init shells out to each of these by name; if any are missing it
    # tries to `brew install` them (which fails inside a Nix shell because
    # `brew` isn't on PATH). xcodegen + ios-deploy live in unstable only.
    pkgsUnstable.xcodegen
    pkgsUnstable.ios-deploy
    cocoapods
    libimobiledevice
    ideviceinstaller
  ];

  shellHook = ''
    export PKG_CONFIG_PATH="${lib.makeSearchPathOutput "dev" "lib/pkgconfig" pkgConfigDeps}:${lib.makeSearchPath "lib/pkgconfig" pkgConfigDeps}:$PKG_CONFIG_PATH"
    export RUST_BACKTRACE=1
    # Apple Team ID for iOS code-signing (consumed by scripts/ios-build.mjs
    # and `tauri ios build`). Matches the `developmentTeam` already set in
    # tauri.conf.json. Override locally if a different signing identity is
    # available — e.g. `APPLE_DEVELOPMENT_TEAM=XXXXXXXXXX npm run dev:ios`.
    export APPLE_DEVELOPMENT_TEAM="''${APPLE_DEVELOPMENT_TEAM:-QDWAV324SU}"
    echo "brilliant-tauri dev shell — run: npm install && npm run tauri dev"
    echo "Rust checks: nix-shell --run 'npm run test:rust' or nix develop .. --command cargo test --manifest-path src-tauri/Cargo.toml"
  '';
}
