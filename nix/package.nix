{
  pkgs,
  inputs,
  system,
  ...
}:
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
    src = ../.;
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
in
craneWindows.buildPackage (
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
)
