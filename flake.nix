{
  description = "Property-based testing for web UIs";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  nixConfig = {
    extra-substituters = "https://bombadil.cachix.org";
    extra-trusted-public-keys = "bombadil.cachix.org-1:6L4epM9zwhEcAwouNgBa8ENtsgLNfedtQgqtdnQhZiM=";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      crane,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = (
          import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          }
        );
        rustToolchainWasm = pkgs.rust-bin.stable.latest.default.override {
          targets = [ "wasm32-unknown-unknown" ];
        };
        wasmBindgenCli =
          let
            version = "0.2.125";
            asset =
              if pkgs.stdenv.isLinux && pkgs.stdenv.isx86_64 then
                "x86_64-unknown-linux-musl"
              else if pkgs.stdenv.isLinux && pkgs.stdenv.isAarch64 then
                "aarch64-unknown-linux-gnu"
              else if pkgs.stdenv.isDarwin && pkgs.stdenv.isAarch64 then
                "aarch64-apple-darwin"
              else if pkgs.stdenv.isDarwin && pkgs.stdenv.isx86_64 then
                "x86_64-apple-darwin"
              else
                throw "wasm-bindgen-cli: unsupported platform";
            hash =
              if pkgs.stdenv.isLinux && pkgs.stdenv.isx86_64 then
                "sha256-Idge90FKClhYYaYOpK4reXDsyu0J1KTgX4vEsVmCfeo="
              else if pkgs.stdenv.isLinux && pkgs.stdenv.isAarch64 then
                "sha256-ed0HMIbqDkf+I/+uAekccpRFtZtB8Ebb+DzY9dmImbA="
              else if pkgs.stdenv.isDarwin && pkgs.stdenv.isAarch64 then
                "sha256-K0b8Aaai9byyTlxekq3yFqOO9PV1QrUpG0T6NPdqxtI="
              else if pkgs.stdenv.isDarwin && pkgs.stdenv.isx86_64 then
                "sha256-fhf2ZWWGpkLVjNd+Y9aW2O+I3xTqRXPEWvaAERF345s="
              else
                throw "wasm-bindgen-cli: unsupported platform";
          in
          pkgs.stdenv.mkDerivation {
            pname = "wasm-bindgen-cli";
            inherit version;
            src = pkgs.fetchurl {
              url = "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${version}/wasm-bindgen-${version}-${asset}.tar.gz";
              inherit hash;
            };
            nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
              pkgs.autoPatchelfHook
            ];
            installPhase = ''
              runHook preInstall
              mkdir -p $out/bin
              install -m755 wasm-bindgen wasm-bindgen-test-runner wasm2es6js $out/bin/
              runHook postInstall
            '';
            meta = {
              description = "CLI for wasm-bindgen, pinned to match Cargo.toml's runtime crate";
              mainProgram = "wasm-bindgen";
            };
          };
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchainWasm;
        craneLibStatic = (crane.mkLib pkgs.pkgsCross.musl64).overrideToolchain (
          p:
          p.rust-bin.stable.latest.default.override {
            targets = [
              "wasm32-unknown-unknown"
              "x86_64-unknown-linux-musl"
            ];
          }
        );
        craneLibAarch64 = (crane.mkLib pkgs.pkgsCross.aarch64-multiplatform-musl).overrideToolchain (
          p:
          p.rust-bin.stable.latest.default.override {
            targets = [
              "wasm32-unknown-unknown"
              "aarch64-unknown-linux-musl"
            ];
          }
        );
        # Pinned to match `GHOSTTY_COMMIT` in libghostty-vt-sys's build.rs at
        # the `libghostty-vt` rev used by `lib/bombadil-terminal/Cargo.toml`.
        # Bump these together when updating libghostty-vt.
        ghosttySrc = pkgs.fetchFromGitHub {
          owner = "ghostty-org";
          repo = "ghostty";
          rev = "fdbf9ff3a31d7531b691cb49c98fc465a1a503a0";
          hash = "sha256-TW2dtJ1wZGtdyqQ4YAsfjbTLURhMISIMNK0c0aIy1xM=";
        };
        bombadil = pkgs.callPackage ./lib/nix/default.nix {
          inherit craneLib craneLibStatic ghosttySrc;
          wasm-bindgen-cli = wasmBindgenCli;
        };
        bombadilAarch64 = pkgs.callPackage ./lib/nix/default.nix {
          inherit craneLib ghosttySrc;
          craneLibStatic = craneLibAarch64;
          wasm-bindgen-cli = wasmBindgenCli;
          cargoTarget = "aarch64-unknown-linux-musl";
        };
      in
      {
        packages = {
          default = bombadil.bin;
          npm-package = bombadil.npm-package;
          manual = pkgs.callPackage ./docs/manual/default.nix { };
          release = pkgs.callPackage ./lib/release/default.nix { };
        }
        // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          aarch64-linux = bombadilAarch64.bin;
          docker = pkgs.callPackage ./lib/nix/docker.nix { bombadil = self.packages.${system}.default; };
        };

        apps = {
          default = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/bombadil";
            meta = self.packages.${system}.default.meta;
          };
        };

        checks = {
          inherit (bombadil) clippy fmt npm-package;
        }
        // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          inherit (bombadil) tests;
        };

        devShells = {
          default = pkgs.mkShell (
            {
              shellHook = ''
                export CC=${pkgs.clang}/bin/clang
                export CXX=${pkgs.clang}/bin/clang++
              '';
              CARGO_INSTALL_ROOT = "${toString ./.}/.cargo";
              inputsFrom = [ self.packages.${system}.default ];
              # nativeBuildInputs takes priority over inputsFrom in
              # PATH, so rustToolchainWasm shadows crane's toolchain.
              nativeBuildInputs = [ rustToolchainWasm ];
              packages = [ (pkgs.callPackage ./lib/nix/cargo-hotpath.nix { }) ];
              buildInputs =
                with pkgs;
                [
                  # Rust
                  rust-analyzer
                  crate2nix
                  cargo-insta

                  # Nix
                  nil

                  # For bombadil-terminal. zig_0_15 / pkg-config come in via
                  # `inputsFrom = [ self.packages.${system}.default ]`; adding
                  # them again here re-sources zig's setup-hook and trips its
                  # readonly `zigDefaultCpuFlag` guard.
                  cmake
                  clang

                  # TS/JS
                  typescript
                  typescript-language-server
                  bun
                  biome

                  # WASM/Inspect UI
                  trunk
                  wasmBindgenCli
                  binaryen

                  # Release automation
                  self.packages.${system}.release
                ]
                ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
                  # Runtime
                  pkgs.chromium
                ];
            }
            // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
              # override how chromiumoxide finds the chromium executable
              CHROME = pkgs.lib.getExe pkgs.chromium;
            }
          );

          manual = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.manual ];
            buildInputs = with pkgs; [
              watchexec
              browser-sync
              concurrently
            ];
            OSFONTDIR = "${pkgs.ibm-plex}/share/fonts/opentype";
          };

          release = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.release ];
          };
        };
      }
    );
}
