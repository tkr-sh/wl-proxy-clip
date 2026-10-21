{
    description = "Wayland clipboard isolation proxy";

    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

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
            rust-overlay,
            crane,
            ...
        }:
        let
            lib = nixpkgs.lib;
            systems = [
                "aarch64-linux"
                "x86_64-linux"
            ];
            eachSystem = lib.genAttrs systems;

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
            overlays.default = final: _prev: {
                wl-proxy-clip = self.packages.${final.stdenv.hostPlatform.system}.default;
            };

            packages = eachSystem (
                system:
                let
                    pkgs = mkPkgs system;
                    craneLib = (crane.mkLib pkgs).overrideToolchain toolchainFor;

                    src = craneLib.cleanCargoSource ./.;
                    inherit (craneLib.crateNameFromCargoToml { inherit src; }) pname version;

                    commonArgs = {
                        inherit src;
                        strictDeps = true;
                    };

                    cargoArtifacts = craneLib.buildDepsOnly commonArgs;
                in
                {
                    default = craneLib.buildPackage (
                        commonArgs
                        // {
                            inherit cargoArtifacts pname version;
                            meta = {
                                description = "Wayland clipboard isolation proxy";
                                homepage = "https://github.com/tkr-sh/wl-proxy-clip";
                                license = lib.licenses.gpl3Plus;
                                mainProgram = "wl-proxy-clip";
                                platforms = lib.platforms.linux;
                            };
                        }
                    );
                }
            );

            apps = eachSystem (
                system:
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
                in
                {
                    default = craneLib.devShell {
                        packages = with pkgs; [
                            just
                            nushell
                            taplo
                            bacon
                            cargo-nextest
                            cargo-machete
                            cargo-deny
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
