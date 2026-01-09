{ pkgs, ... }:
pkgs.mkShell rec {
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
}
