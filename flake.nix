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
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, bun2nix, git-hooks }:
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

        pre-commit-check = git-hooks.lib.${system}.run {
          src = ./.;
          hooks = {
            # === General checks ===
            check-merge-conflicts.enable = true;
            check-added-large-files = {
              enable = true;
              stages = [ "pre-commit" ];
            };
            detect-private-keys.enable = true;

            # === File format validation ===
            check-json.enable = true;
            check-yaml.enable = true;
            check-toml.enable = true;

            # === Spelling ===
            typos = {
              enable = true;
              excludes = [
                "^frontend/bun\\.nix$"
                "^frontend/bun\\.lock$"
                "^docs/.*$"
              ];
              settings = {
                ignored-words = [
                  "ser"
                  "idents"
                ];
              };
            };

            # === Nix ===
            nixfmt.enable = true;
            deadnix.enable = true;
            statix.enable = true;

            # === Rust ===
            rustfmt = {
              enable = true;
              name = "rustfmt";
              entry = "bash -c 'cd backend && cargo fmt --check'";
              files = "^backend/.*\\.rs$";
              pass_filenames = false;
              language = "system";
            };

            clippy = {
              enable = true;
              name = "clippy";
              entry = "bash -c 'cd backend && cargo clippy -- -D warnings'";
              files = "^backend/.*\\.rs$";
              pass_filenames = false;
              language = "system";
            };

            # === Frontend bun.nix sync check ===
            frontend-bun-nix-check = {
              enable = true;
              name = "frontend-bun-nix-check";
              entry = toString (
                pkgs.writeShellScript "frontend-bun-nix-check" ''
                  set -e
                  cd frontend

                  ${pkgs.bun}/bin/bunx bun2nix -o bun.nix 2>/dev/null

                  if ! git diff --exit-code --quiet bun.nix 2>/dev/null; then
                    echo "ERROR: frontend/bun.nix is out of date!"
                    echo ""
                    echo "Run: cd frontend && bunx bun2nix -o bun.nix"
                    echo "Then stage the updated bun.nix."
                    exit 1
                  fi
                ''
              );
              files = "^frontend/bun\\.lock$";
              pass_filenames = false;
              language = "system";
            };
          };
        };
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
          inherit (pre-commit-check) shellHook;
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
