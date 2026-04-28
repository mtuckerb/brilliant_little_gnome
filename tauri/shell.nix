# Dev shell for the Tauri port. Usage: `nix-shell` from this directory.
# Cross-platform: works on NixOS (Linux) and nix-darwin (macOS). Tauri's
# webview backend pulls different system libs per OS, so we gate them on
# `stdenv.isLinux` / `stdenv.isDarwin`.
{ pkgs ? import <nixpkgs> {} }:

let
  inherit (pkgs) lib stdenv;
in
pkgs.mkShell {
  buildInputs = with pkgs; [
    # Rust toolchain
    cargo
    rustc
    rustfmt
    clippy
    rust-analyzer

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
    # Tauri/macOS uses the system WebKit, but Rust crates often need these.
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
