{
  pkgs ? import <nixpkgs> { },
}:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # For MSVC target cross-compilation with cargo-xwin
    llvmPackages_latest.clang
    llvmPackages_latest.lld
    llvmPackages_latest.llvm # Includes llvm-lib
    wine64 # Optional: for testing the .exe
  ];

  # Make clang-cl available (clang with MSVC compatibility)
  shellHook = ''
    echo "Cross-compilation shell for Windows x86_64 (MSVC target)"
    echo ""
    echo "Build with:"
    echo "  RUSTFLAGS=\"-C target-feature=+crt-static\" cargo xwin build --release --target x86_64-pc-windows-msvc"
    echo ""
    echo "Output will be at: target/x86_64-pc-windows-msvc/release/main.exe"
  '';
}
