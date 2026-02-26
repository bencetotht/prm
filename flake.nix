{
  description = "prm - terminal-first project repository manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachSystem [
      "x86_64-linux"
      "aarch64-linux"
      "x86_64-darwin"
      "aarch64-darwin"
    ] (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        packageName = cargoToml.package.name;
        packageVersion = cargoToml.package.version;
      in
      {
        packages.default = rustPlatform.buildRustPackage {
          pname = packageName;
          version = packageVersion;
          src = ./.;

          nativeCheckInputs = [ pkgs.git ];

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

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/${packageName}";
          meta = self.packages.${system}.default.meta;
        };

        checks.default = self.packages.${system}.default;

        devShells.default = pkgs.mkShell {
          packages = [ rustToolchain ];
        };
      });
}
