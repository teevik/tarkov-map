{
  pkgs,
  inputs,
  system,
  ...
}:
let
  inherit (pkgs) lib;

  target = "x86_64-pc-windows-msvc";
  envTarget = builtins.replaceStrings [ "-" ] [ "_" ] target;

  fenixPkgs = inputs.fenix.packages.${system};
  rustToolchain = fenixPkgs.combine [
    fenixPkgs.stable.rustc
    fenixPkgs.stable.cargo
    fenixPkgs.targets.${target}.stable.rust-std
  ];

  craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;

  # Pre-packaged Windows SDK/CRT (generated with xwin)
  wlibs = pkgs.runCommand "windows-libs" { } ''
    ${pkgs.xz}/bin/xz -d < ${./windows-libs.tar.xz} | ${pkgs.gnutar}/bin/tar -x
    mv windows-libs $out
  '';

  clangVersion = lib.versions.major pkgs.libclang.version;
  includePaths = [
    "${pkgs.libclang.lib}/lib/clang/${clangVersion}/include"
    "${wlibs}/crt/include"
    "${wlibs}/sdk/include/ucrt"
    "${wlibs}/sdk/include/um"
    "${wlibs}/sdk/include/shared"
  ];
  libPaths = [
    "${wlibs}/crt/lib/x86_64"
    "${wlibs}/sdk/lib/ucrt/x86_64"
    "${wlibs}/sdk/lib/um/x86_64"
  ];

  # ==========================================================================
  # Layer 1: Source filtering (separate Cargo sources from assets)
  # ==========================================================================

  # Cargo sources only (for dependency resolution and vendoring)
  cargoSource = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      (craneLib.filterCargoSources path type) || builtins.match ".*schema\\.graphql$" path != null;
  };

  # Full source including assets (for final build)
  fullSource = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      (craneLib.filterCargoSources path type)
      || builtins.match ".*schema\\.graphql$" path != null
      || builtins.match ".*/assets/.*" path != null;
  };

  # ==========================================================================
  # Layer 2: Vendored dependencies (cached when Cargo.lock unchanged)
  # ==========================================================================

  cargoVendorDir = craneLib.vendorCargoDeps {
    src = cargoSource;
  };

  # ==========================================================================
  # Common build arguments
  # ==========================================================================

  commonArgs = {
    pname = "tarkov-map";
    version = "0.1.0";
    strictDeps = true;
    doCheck = false;

    inherit cargoVendorDir;

    nativeBuildInputs = with pkgs; [
      clang
      lld
      llvm
    ];
    CARGO_BUILD_TARGET = target;

    "CC_${envTarget}" = "clang-cl";
    "CXX_${envTarget}" = "clang-cl";
    "AR_${envTarget}" = "llvm-lib";
    "CARGO_TARGET_${lib.toUpper envTarget}_LINKER" = "lld-link";
    "CARGO_TARGET_${lib.toUpper envTarget}_RUSTFLAGS" = lib.concatStringsSep " " (
      [
        "-Clinker-flavor=lld-link"
        "-Ctarget-feature=+crt-static"
      ]
      ++ map (p: "-Lnative=${p}") libPaths
    );
    "CFLAGS_${envTarget}" = lib.concatStringsSep " " (
      [
        "--target=${target}"
        "-fuse-ld=lld-link"
      ]
      ++ map (p: "/imsvc${p}") includePaths
    );
    "BINDGEN_EXTRA_CLANG_ARGS_${envTarget}" = lib.concatMapStringsSep " " (p: "-I${p}") includePaths;

    dontFixup = true;
  };

  # ==========================================================================
  # Layer 3: Compiled dependencies (cached when deps unchanged)
  # ==========================================================================

  cargoArtifacts = craneLib.buildDepsOnly (
    commonArgs
    // {
      src = cargoSource;
    }
  );

  # ==========================================================================
  # Layer 4: Final package (rebuilds only when your code changes)
  # ==========================================================================
in
craneLib.buildPackage (
  commonArgs
  // {
    src = fullSource;
    inherit cargoArtifacts;
    cargoExtraArgs = "--bin main";
    installPhaseCommand = ''
      mkdir -p $out/bin
      cp target/${target}/release/main.exe $out/bin/tarkov-map.exe
    '';
  }
)
