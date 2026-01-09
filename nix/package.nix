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

  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      (craneLib.filterCargoSources path type)
      || builtins.match ".*schema\\.graphql$" path != null
      || builtins.match ".*/assets/.*" path != null;
  };

  commonArgs = {
    pname = "tarkov-map";
    version = "0.1.0";
    inherit src;
    strictDeps = true;
    doCheck = false;

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

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoExtraArgs = "--bin main";
    installPhaseCommand = ''
      mkdir -p $out/bin
      cp target/${target}/release/main.exe $out/bin/tarkov-map.exe
    '';
  }
)
