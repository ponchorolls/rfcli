{
  description = "rfcli: A fast RFC reader with fuzzy search and TLDR";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "rfcli";
          version = "0.1.0";

          src = ./.;

          # This hash needs to be updated if you change dependencies in Cargo.toml
          # You can set it to lib.fakeSha256 first, run nix build, and copy the real hash
          cargoHash = "sha256-FdnQs2qbDEPGcyCbWpK8YEid+Vx6OfV213YX15HImaQ="; 

          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ openssl ];

          meta = with pkgs.lib; {
            description = "RFC CLI tool with fuzzy search and TLDR";
            license = licenses.mit;
            maintainers = [ ];
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustc cargo rust-analyzer pkg-config openssl bat
          ];
        };
      });
}
