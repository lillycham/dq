{
  description = "dq — directory query, like jq for directories";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }: let
    systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
    forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
  in {
    packages = forAllSystems (pkgs: {
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "dq";
        version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
        src = pkgs.lib.cleanSource ./.;
        cargoLock.lockFile = ./Cargo.lock;
        meta = {
          description = "Query and manipulate directory trees with jq expressions";
          license = pkgs.lib.licenses.gpl3Plus;
          mainProgram = "dq";
        };
      };
    });

    devShells = forAllSystems (pkgs: {
      default = pkgs.mkShell {
        inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.default ];
        packages = with pkgs; [ cargo rustc clippy rustfmt rust-analyzer ];
      };
    });

    checks = forAllSystems (pkgs: {
      # Building the package also runs `cargo test`.
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
    });

    formatter = forAllSystems (pkgs: pkgs.nixfmt-rfc-style);
  };
}
