{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    inputs:
    let
      # The systems supported for this flake
      supportedSystems = [
        "x86_64-linux" # 64-bit Intel/AMD Linux
        "aarch64-linux" # 64-bit ARM Linux
        "x86_64-darwin" # 64-bit Intel macOS
        "aarch64-darwin" # 64-bit ARM macOS
      ];

      forEachSupportedSystem =
        f:
        inputs.nixpkgs.lib.genAttrs supportedSystems (
          system:
          f {
            inherit system;
            pkgs = import inputs.nixpkgs { inherit system; };
          }
        );
    in
    {
      devShells = forEachSupportedSystem (
        { pkgs, system }:
        {
          default = pkgs.mkShell rec {
            packages = with pkgs; [
              # wayland
              wayland

              # x11
              xorg.libX11
              xorg.libXrandr
              xorg.libXinerama
              xorg.libXcursor
              xorg.libXi
              xorg.libxcb
              libxkbcommon

              # opengl
              libGL
            ];

            LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath packages}";
          };

          # Shell for Windows cross-compilation
          cross-windows = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc

              # LLVM toolchain for cross-compilation
              llvmPackages_latest.clang
              llvmPackages_latest.lld
              llvmPackages_latest.llvm

              # cargo-xwin for downloading Windows SDK
              cargo-xwin

              # Optional: for testing
              wine64
            ];

            shellHook = ''
              echo "Cross-compilation shell for Windows x86_64 (MSVC target)"
              echo ""
              echo "First, add the Windows target:"
              echo "  rustup target add x86_64-pc-windows-msvc"
              echo ""
              echo "Then build with:"
              echo "  RUSTFLAGS=\"-C target-feature=+crt-static\" cargo xwin build --release --target x86_64-pc-windows-msvc"
              echo ""
              echo "Or use the flake package:"
              echo "  nix build .#tarkov-map-windows"
            '';
          };
        }
      );

      packages = forEachSupportedSystem (
        { pkgs, system }:
        let
          inherit (pkgs.lib) concatStringsSep;
          inherit (pkgs.lib.strings) toUpper;

          # Use fenix for Rust with Windows target support
          fenixPkgs = inputs.fenix.packages.${system};

          # Rust toolchain with Windows MSVC target
          rustWindows = fenixPkgs.combine [
            fenixPkgs.stable.rustc
            fenixPkgs.stable.cargo
            fenixPkgs.targets.x86_64-pc-windows-msvc.stable.rust-std
          ];

          craneLib = inputs.crane.mkLib pkgs;
          craneWindows = craneLib.overrideToolchain rustWindows;

          # Filter source to only include necessary files
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type)
              || (builtins.match ".*schema\.graphql$" path != null)
              || (builtins.match ".*/assets/.*" path != null);
          };

          # Windows target configuration
          target = "x86_64-pc-windows-msvc";
          envTarget = builtins.replaceStrings [ "-" ] [ "_" ] target;

          # Pre-packaged Windows SDK and CRT libraries
          # Generated with: xwin --accept-license splat --output windows-libs
          # Then: tar -cJf windows-libs.tar.xz windows-libs
          wlibs = pkgs.stdenvNoCC.mkDerivation {
            name = "windows-libs";
            src = ./windows-libs.tar.xz;
            sourceRoot = ".";
            installPhase = "mkdir $out && cp -R windows-libs/* $out";
            phases = [
              "unpackPhase"
              "installPhase"
            ];
          };

          # Include paths for clang
          libs = [
            "${pkgs.libclang.lib}/lib/clang/${pkgs.lib.versions.major pkgs.libclang.version}/include"

            "${wlibs}"
            "${wlibs}/sdk/lib"
            "${wlibs}/sdk/include"
            "${wlibs}/sdk/include/um"
            "${wlibs}/sdk/include/ucrt"
            "${wlibs}/sdk/include/shared"

            "${wlibs}/crt/lib"
            "${wlibs}/crt/include"

            "${wlibs}/crt/lib/x86_64"
            "${wlibs}/sdk/lib/um/x86_64"
            "${wlibs}/sdk/lib/ucrt/x86_64"
          ];

          # Common build configuration for Windows target
          commonWindows = {
            pname = "tarkov-map";
            version = "0.1.0";
            inherit src;

            strictDeps = true;
            doCheck = false;

            buildInputs = with pkgs; [
              clang
              libclang
              libclang.lib
              libllvm
              lld
              wlibs
            ];

            nativeBuildInputs = with pkgs; [ pkg-config ] ++ commonWindows.buildInputs;
            depsBuildBuild = commonWindows.nativeBuildInputs;

            # Use target-specific RUSTFLAGS to avoid affecting host rustc invocations
            "CARGO_TARGET_${toUpper envTarget}_RUSTFLAGS" = concatStringsSep " " (
              [
                "-Clinker-flavor=lld-link"
                "-Ctarget-feature=+crt-static"
              ]
              ++ map (l: "-Lnative=${l}") libs
            );

            CARGO_BUILD_TARGET = target;
            "CARGO_TARGET_${toUpper envTarget}_LINKER" = "lld-link";

            TARGET_CC = "clang-cl";
            TARGET_CXX = "clang-cl";
            "CC_${envTarget}" = "clang-cl";
            "CXX_${envTarget}" = "clang-cl";

            TARGET_AR = "llvm-lib";
            "AR_${envTarget}" = "llvm-lib";

            CL_FLAGS = concatStringsSep " " (
              [
                "--target=${target}"
                "-Wno-unused-command-line-argument"
                "-fuse-ld=lld-link"
              ]
              ++ map (str: "/imsvc${str}") libs
            );
            "CFLAGS_${envTarget}" = commonWindows.CL_FLAGS;
            "CXXFLAGS_${envTarget}" = commonWindows.CL_FLAGS;

            RC_FLAGS = concatStringsSep " " ([ ] ++ map (str: "-I${str}") libs);
            "BINDGEN_EXTRA_CLANG_ARGS_${envTarget}" = commonWindows.RC_FLAGS;

            dontFixup = true;
            dontStrip = true;
          };

          # Build dependencies separately for caching (Windows)
          cargoArtifactsWindows = craneWindows.buildDepsOnly commonWindows;

          # Windows build
          tarkov-map-windows = craneWindows.buildPackage (
            commonWindows
            // {
              cargoArtifacts = cargoArtifactsWindows;

              # Only build the main binary
              cargoExtraArgs = "--bin main";

              # Override install phase to copy and rename the executable
              installPhaseCommand = ''
                mkdir -p $out/bin
                cp target/${target}/release/main.exe $out/bin/tarkov-map.exe
              '';
            }
          );

        in
        {
          default = tarkov-map-windows;
          tarkov-map-windows = tarkov-map-windows;
        }
      );
    };
}
