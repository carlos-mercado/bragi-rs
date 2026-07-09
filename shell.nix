{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  buildInputs = [
    pkgs.chafa
    pkgs.pkg-config
    pkgs.rustup
    pkgs.glib
    pkgs.alsa-utils 
    pkgs.pipewire
  ];
  shellHook = ''
    export ALSA_PLUGIN_DIR="${pkgs.pipewire}/lib/alsa-lib"
  '';
}
