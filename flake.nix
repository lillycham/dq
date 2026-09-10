{
  description = "dq — directory query, like jq for directories";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }: let
    systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
  in {
    packages = nixpkgs.lib.genAttrs systems (system: {
      default = nixpkgs.legacyPackages.${system}.rustPlatform.buildRustPackage {
        pname = "dq";
        version = "0.1.0";
        src = ./.;
        cargoHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
      };
    });
  };
}
