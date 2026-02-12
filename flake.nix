{
  description = "Mercy - Mercenary Exchange Locator Service";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    bun2nix = {
      url = "github:nix-community/bun2nix";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, bun2nix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.stable.latest.default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        bun2nixPkg = bun2nix.packages.${system}.default;
      in {
        packages = {
          mercy-backend = rustPlatform.buildRustPackage {
            pname = "mercy";
            version = "0.1.0";
            src = ./backend;
            cargoLock.lockFile = ./backend/Cargo.lock;
            nativeBuildInputs = with pkgs; [ pkg-config ];
            buildInputs = with pkgs; [ openssl ];

            postInstall = ''
              mkdir -p $out/share/mercy/assets
              cp -r assets/* $out/share/mercy/assets/
            '';
          };

          mercy-frontend = pkgs.stdenv.mkDerivation {
            pname = "mercy-frontend";
            version = "0.1.0";
            src = ./frontend;

            nativeBuildInputs = [
              bun2nixPkg.hook
            ];

            bunDeps = bun2nixPkg.fetchBunDeps {
              bunNix = ./frontend/bun.nix;
              useFakeNode = false;
            };

            dontCheckForBrokenSymlinks = true;

            buildPhase = ''
              runHook preBuild
              export MERCY_BACKEND_URL=""
              bun run build
              runHook postBuild
            '';

            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp .next/standalone/server.js $out/
              cp .next/standalone/package.json $out/
              cp -r .next/standalone/.next $out/.next
              cp -r .next/static $out/.next/static
              if [ -d ".next/standalone/node_modules" ]; then
                cp -r .next/standalone/node_modules $out/node_modules
              fi
              cp cache-handler.js $out/
              if [ -d "public" ]; then cp -r public $out/public; fi
              test -f "$out/server.js" || { echo "ERROR: server.js missing"; exit 1; }
              runHook postInstall
            '';
          };

          default = self.packages.${system}.mercy-backend;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            pkg-config
            openssl
            bun
            just
            (python3.withPackages (ps: [ ps.pillow ]))
            (writeShellScriptBin "dev" "just dev")
            (writeShellScriptBin "stop" "just stop")
          ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            pkgs.chromium
          ];
        };
      }
    ) // {
      nixosModules.default = import ./nix/module.nix;
    };
}
