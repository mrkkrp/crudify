{
  description = "Reduce resolution and number of colors in preparation for painting";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rustfmt" "clippy" ];
        };
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        src = craneLib.cleanCargoSource ./.;
        commonArgs = {
          inherit src;
          strictDeps = true;
        };
        # Build only the dependencies once so they can be reused across the
        # package build and every check.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        crudify = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });
      in
      {
        checks = {
          inherit crudify;
          crudify-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
          crudify-doc = craneLib.cargoDoc (commonArgs // {
            inherit cargoArtifacts;
          });
          crudify-fmt = craneLib.cargoFmt {
            inherit src;
          };
          crudify-nextest = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
            cargoNextestExtraArgs = "--no-tests=pass";
          });
        };
        packages.default = crudify;
        apps.default = flake-utils.lib.mkApp {
          drv = crudify;
        };
        devShells.default = craneLib.devShell {
          checks = self.checks.${system};
          packages = [
            pkgs.rust-analyzer
          ];
        };
      }
    );
}
