{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
        moq-relay = pkgs.rustPlatform.buildRustPackage {
          pname = "moq-relay";
          version = "0.12.0";
          src = pkgs.fetchFromGitHub {
            owner = "moq-dev";
            repo = "moq";
            rev = "78bc805a7683afdcaa1135a7dbebec6359b8528a";
            hash = "sha256-bVl9xVG+b0IrlaLV6QUZmYKFx7cUu5W1F1MQLHb30FA=";
          };
          cargoHash = "sha256-mmo9Ki7oTjXnxUZNLwdENGeMG8DX/8U3ke59FYZUfQ0=";
          cargoBuildFlags = [ "-p" "moq-relay" ];
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.apple-sdk_15
            pkgs.libiconv
          ];
          doCheck = false;
        };
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rust
            pkgs.protobuf
            pkgs.just
            pkgs.cargo-watch
            pkgs.jujutsu
            pkgs.git
            moq-relay
          ];

          shellHook = ''
            echo "MoQ Prototype Development Environment"
            echo ""
            echo "Run 'just' to see available commands"
          '';
        };
      });
}
