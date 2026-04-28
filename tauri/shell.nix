# Dev shell for the Tauri port. Usage: `nix-shell` from this directory.
# Cross-platform: NixOS (Linux) + nix-darwin (macOS). Pulls a fixed recent
# stable Rust via oxalica/rust-overlay so both machines agree on the
# toolchain regardless of which channel the host nixpkgs is on.

let
  rustOverlay = import (fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/master.tar.gz";
  });
  pkgs = import <nixpkgs> { overlays = [ rustOverlay ]; };
  inherit (pkgs) lib stdenv;

  # Single source of truth for the toolchain. Bump as needed.
  rustToolchain = pkgs.rust-bin.stable."1.90.0".default.override {
    extensions = [ "rust-src" "rustfmt" "clippy" "rust-analyzer" ];
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
  ];

  shellHook = ''
    export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
    export RUST_BACKTRACE=1
    echo "brilliant-tauri dev shell — run: npm install && npm run tauri dev"
  '';
}
