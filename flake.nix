{
    description = "Project flake";

    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
        systems.url = "github:nix-systems/default";

        rust-overlay = {
            url = "github:oxalica/rust-overlay";
            inputs.nixpkgs.follows = "nixpkgs";
        };

        crane = {
            url = "github:ipetkov/crane";
            inputs.nixpkgs.follows = "nixpkgs";
        };
    };

    outputs =
        {
            self,
            nixpkgs,
            systems,
            rust-overlay,
            crane,
            ...
        }:
        let
            lib = nixpkgs.lib;
            eachSystem = lib.genAttrs (import systems);

            mkPkgs =
                system:
                import nixpkgs {
                    inherit system;
                    overlays = [ (import rust-overlay) ];
                };

            # Define the toolchain once
            toolchainFor =
                pkgs:
                pkgs.rust-bin.nightly.latest.default.override {
                    extensions = [
                        "rust-src"
                        "rust-analyzer"
                        "clippy"
                        "rustfmt"
                    ];
                };
        in
        {
            overlays.default =
                final: _prev:
                let
                    craneLib = (crane.mkLib final).overrideToolchain toolchainFor;
                    src = craneLib.cleanCargoSource ./.;
                    inherit (craneLib.crateNameFromCargoToml { inherit src; }) pname;
                in
                {
                    ${pname} = self.packages.${final.stdenv.hostPlatform.system}.default;
                };

            packages = eachSystem (
                system:
                let
                    pkgs = mkPkgs system;
                    craneLib = (crane.mkLib pkgs).overrideToolchain toolchainFor;

                    src = craneLib.cleanCargoSource ./.;
                    # Automatically read pname and version from Cargo.toml
                    inherit (craneLib.crateNameFromCargoToml { inherit src; }) pname version;

                    commonArgs = {
                        inherit src;
                        strictDeps = true;
                        nativeBuildInputs = [ pkgs.pkg-config ];
                        buildInputs = [ pkgs.openssl ];
                    };

                    cargoArtifacts = craneLib.buildDepsOnly commonArgs;

                    my-crate = craneLib.buildPackage (
                        commonArgs
                        // {
                            inherit cargoArtifacts pname version;
                            doCheck = false; # Tests run via `just`, not nix
                        }
                    );
                in
                {
                    default = my-crate;
                }
            );

            apps = eachSystem (
                system:
                let
                    pkgs = mkPkgs system;
                in
                {
                    default = {
                        type = "app";
                        program = lib.getExe self.packages.${system}.default;
                    };
                }
            );

            devShells = eachSystem (
                system:
                let
                    pkgs = mkPkgs system;
                    craneLib = (crane.mkLib pkgs).overrideToolchain toolchainFor;

                    commonArgs = {
                        src = craneLib.cleanCargoSource ./.;
                        strictDeps = true;
                        nativeBuildInputs = [ pkgs.pkg-config ];
                        buildInputs = [ pkgs.openssl ];
                    };
                in
                {
                    default = craneLib.devShell {
                        inherit (commonArgs) buildInputs nativeBuildInputs;
                        packages = with pkgs; [
                            just
                            nushell
                            taplo
                            bacon
                            cargo-nextest
                            cargo-machete
                            fd
                            ripgrep
                            nixfmt
                        ];
                        env.RUST_SRC_PATH = "${toolchainFor pkgs}/lib/rustlib/src/rust/library";
                    };
                }
            );

            formatter = eachSystem (
                system:
                let
                    pkgs = mkPkgs system;
                in
                pkgs.writeShellApplication {
                    name = "nixfmt";
                    runtimeInputs = [ pkgs.nixfmt ];
                    text = ''
                        exec nixfmt --indent 4 "$@"
                    '';
                }
            );
        };
}
