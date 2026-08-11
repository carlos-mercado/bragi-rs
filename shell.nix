{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  buildInputs = [
    pkgs.chafa
    pkgs.pkg-config
    pkgs.rustup
    pkgs.glib
    pkgs.alsa-utils 
    pkgs.alsa-lib
    pkgs.pipewire
    
    # Correct Rust profiling tools
    pkgs.cargo-flamegraph
    pkgs.linuxPackages.perf
  ];
  shellHook = ''
    export ALSA_PLUGIN_DIR="${pkgs.pipewire}/lib/alsa-lib"
  '';
}
