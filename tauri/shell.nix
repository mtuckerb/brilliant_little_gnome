# Dev shell for the Tauri port. Usage: `nix-shell` from this directory.
# Cross-platform: NixOS (Linux) + nix-darwin (macOS). Pulls a fixed recent
# stable Rust via oxalica/rust-overlay so both machines agree on the
# toolchain regardless of which channel the host nixpkgs is on.

let
  # Pin nixpkgs explicitly so we don't depend on the host having a
  # `nixpkgs` channel registered (the macOS multi-user installer skips
  # that step by default, which makes `<nixpkgs>` resolution fail).
  nixpkgsTarball = fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/nixos-24.05.tar.gz";
  };
  rustOverlay = import (fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/master.tar.gz";
  });
  pkgs = import nixpkgsTarball { overlays = [ rustOverlay ]; };
  inherit (pkgs) lib stdenv;

  # `xcodegen` only landed in nixpkgs unstable (not in 24.05 or 24.11), so
  # we pin a separate unstable just for that single package and pull the
  # rest of the toolchain from the stable pin above.
  pkgsUnstable = import (fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/nixos-unstable.tar.gz";
  }) {};

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
in
pkgs.mkShell {
  buildInputs = with pkgs; [
    rustToolchain

    # Node + build tools (cross-platform)
    nodejs_20
    pkg-config
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
    export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
    export RUST_BACKTRACE=1
    echo "brilliant-tauri dev shell — run: npm install && npm run tauri dev"
  '';
}
