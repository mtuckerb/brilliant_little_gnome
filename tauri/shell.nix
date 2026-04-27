# Dev shell for the Tauri port. Usage: `nix-shell` from this directory.
# Pulls in the system libs Tauri's webview backend needs on Linux.
{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # Toolchains
    cargo
    rustc
    rust-analyzer
    nodejs_20
    pkg-config

    # Tauri linux runtime deps
    webkitgtk_4_1
    gtk3
    cairo
    gdk-pixbuf
    glib
    pango
    harfbuzz
    libsoup_3
    openssl
    librsvg
    atk

    # Build tools
    gcc
  ];

  shellHook = ''
    export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
    export RUST_BACKTRACE=1
    echo "brilliant-tauri dev shell — run: npm install && npm run tauri dev"
  '';
}
