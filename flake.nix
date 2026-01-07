{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  # Flake outputs
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
            pkgs = import inputs.nixpkgs { inherit system; };
          }
        );
    in
    {
      devShells = forEachSupportedSystem (
        { pkgs }:
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

            # # Set any environment variables for your dev shell
            # env = { };

            # # Add any shell logic you want executed any time the environment is activated
            # shellHook = '''';
          };
        }
      );
    };
}
