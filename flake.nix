{
  description = "Build a cargo workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane";

    flake-utils.url = "github:numtide/flake-utils";

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      flake-utils,
      advisory-db,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        inherit (pkgs) lib;

        craneLib = crane.mkLib pkgs;
        src = craneLib.cleanCargoSource ./.;

        # Common arguments can be set here to avoid repeating them later
        baseArgs = {
          nativeBuildInputs = [
            pkgs.rustPlatform.bindgenHook
            # Add additional build inputs here
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [
            (pkgs.writeShellScriptBin "sw_vers" ''
              echo 'ProductVersion: ${pkgs.stdenv.hostPlatform.darwinMinVersion}'
            '')

            # Additional darwin specific inputs can be set here
          ];

          inherit src;

        };
        cargoVendorDir = craneLib.vendorCargoDeps (
          baseArgs
          // {
            overrideVendorCargoPackage =
              p: drv:
              if p.name == "karabiner-driverkit" then
                drv.overrideAttrs (old: {
                  #         env.NIX_CFLAGS_COMPILE =
                  #           builtins.trace old old.env.NIX_CFLAGS_COMPILE or "" + " -I${old.src}/c_src/include";
                  # This is a bit of a hack to work around the fact that this crate
                  # has a build.rs that tries to link to a C library that we don't
                  # actually need, since we're only building the Rust code and not
                  # running any build scripts. By overriding the build inputs to be
                  # empty, we can avoid the build.rs from trying to link to the C library.
                })
              else
                drv;

          }
        );

        commonArgs = baseArgs // {

          strictDeps = true;

          inherit cargoVendorDir;

          # Additional environment variables can be set directly
          # MY_CUSTOM_VAR = "some value";
        };

        # Build *just* the cargo dependencies (of the entire workspace),
        # so we can reuse all of that work (e.g. via cachix) when running in CI
        # It is *highly* recommended to use something like cargo-hakari to avoid
        # cache misses when building individual top-level-crates
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        individualCrateArgs = commonArgs // {
          inherit cargoArtifacts;
          inherit (craneLib.crateNameFromCargoToml { inherit src; }) version;
          # NB: we disable tests since we'll run them all via cargo-nextest
          doCheck = false;
        };

        fileSetForCrate =
          crate:
          lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              # Also keep any markdown files
              (craneLib.fileset.commonCargoSources crate)
            ];
          };

        members = [
          "parser"
          "kerberon"
          "example_tcp_client"
          "tcp_protocol"
          "windows_key_tester"
          "simulated_input"
          "simulated_passthru"
        ];
        kanata = pkgs.callPackage ./package.nix { inherit craneLib individualCrateArgs fileSetForCrate; };
        other_members = lib.genAttrs' members (member: {
          name = member;
          value = craneLib.buildPackage (
            individualCrateArgs
            // {
              pname = member;
              cargoExtraArgs = "-p ${member}";
              src = fileSetForCrate ./.;
            }
          );
        });
      in
      {
        checks = {
          # Build the crates as part of `nix flake check` for convenience
          inherit kanata;
          inherit (other_members)
            parser
            kerberon
            example_tcp_client
            tcp_protocol
            windows_key_tester
            simulated_input
            simulated_passthru
            ;

          # Run clippy (and deny all warnings) on the workspace source,
          # again, reusing the dependency artifacts from above.
          #
          # Note that this is done as a separate derivation so that
          # we can block the CI if there are issues here, but not
          # prevent downstream consumers from building our crate by itself.
          my-workspace-clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );

          my-workspace-doc = craneLib.cargoDoc (
            commonArgs
            // {
              inherit cargoArtifacts;
              # This can be commented out or tweaked as necessary, e.g. set to
              # `--deny rustdoc::broken-intra-doc-links` to only enforce that lint
              env.RUSTDOCFLAGS = "--deny warnings";
            }
          );

          # Check formatting
          my-workspace-fmt = craneLib.cargoFmt {
            inherit src;
          };

          my-workspace-toml-fmt = craneLib.taploFmt {
            src = pkgs.lib.sources.sourceFilesBySuffices src [ ".toml" ];
            # taplo arguments can be further customized below as needed
            # taploExtraArgs = "--config ./taplo.toml";
          };

          # Audit dependencies
          my-workspace-audit = craneLib.cargoAudit {
            inherit src advisory-db;
          };

          # Audit licenses
          my-workspace-deny = craneLib.cargoDeny {
            inherit src;
          };

          # Run tests with cargo-nextest
          # Consider setting `doCheck = false` on other crate derivations
          # if you do not want the tests to run twice
          my-workspace-nextest = craneLib.cargoNextest (
            commonArgs
            // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
              cargoNextestPartitionsExtraArgs = "--no-tests=pass";
            }
          );

          # Ensure that cargo-hakari is up to date
          my-workspace-hakari = craneLib.mkCargoDerivation {
            inherit src;
            pname = "my-workspace-hakari";
            cargoArtifacts = null;
            doInstallCargoArtifacts = false;

            buildPhaseCargoCommand = ''
              cargo hakari generate --diff  # workspace-hack Cargo.toml is up-to-date
              cargo hakari manage-deps --dry-run  # all workspace crates depend on workspace-hack
              cargo hakari verify
            '';

            nativeBuildInputs = [
              pkgs.cargo-hakari
            ];
          };
        };

        packages = {
          inherit kanata;
        };

        apps = {
          kanata = flake-utils.lib.mkApp {
            drv = kanata;
          };
        };

        devShells.default = craneLib.devShell {
          # Inherit inputs from checks.
          checks = self.checks.${system};

          # Additional dev-shell environment variables can be set directly
          # MY_CUSTOM_DEVELOPMENT_VAR = "something else";

          # Extra inputs can be added here; cargo and rustc are provided by default.
          packages = [
            pkgs.cargo-hakari
            pkgs.rust-analyzer
            pkgs.cargo-edit
          ];
        };
      }
    );
}
