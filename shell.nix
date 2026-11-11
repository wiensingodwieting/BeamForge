{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustc
    cargo
    pkg-config
    fontconfig
    freetype
    libxkbcommon
    wayland
    libx11
    libxcursor
    libxi
    libxrandr
    libGL
  ];

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
    libGL
    libxkbcommon
    wayland
    libx11
    libxcursor
    libxi
    libxrandr
    fontconfig
    freetype
  ]);
}
