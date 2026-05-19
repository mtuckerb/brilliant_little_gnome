{
  description = "Brilliant development shells";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, nixpkgs-unstable }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        pkgsUnstable = import nixpkgs-unstable { inherit system; };
        tauriShell = import ./tauri/shell.nix { inherit system pkgs pkgsUnstable; };
      in
      {
        devShells = {
          default = tauriShell;
          tauri = tauriShell;
        };
      });
}
