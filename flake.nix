{
  description = "prm - terminal-first project repository manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachSystem [
      "x86_64-linux"
      "aarch64-linux"
      "x86_64-darwin"
      "aarch64-darwin"
    ] (system:
      let
        pkgs = import nixpkgs { inherit system; };
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        packageName = cargoToml.package.name;
        packageVersion = cargoToml.package.version;
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = packageName;
          version = packageVersion;
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = with pkgs.lib; {
            description = cargoToml.package.description;
            homepage = cargoToml.package.homepage;
            license = [ licenses.mit licenses.asl20 ];
            mainProgram = packageName;
            platforms = platforms.unix;
          };
        };

        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };

        checks.default = self.packages.${system}.default;

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            clippy
            rustc
            rustfmt
          ];
        };
      });
}
