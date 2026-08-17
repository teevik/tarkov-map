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

    # vulkan (wgpu backend; the nixpkgs loader finds system ICDs in
    # /run/opengl-driver/share on NixOS)
    vulkan-loader
  ];

  LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath packages}";
}
