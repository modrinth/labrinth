# TODO: Move to flake
{pkgs ? import <nixpkgs> {},
 fenix ? import (fetchTarball "https://github.com/nix-community/fenix/archive/main.tar.gz") {}
}:
pkgs.mkShell {
  buildInputs = with pkgs; [
    fenix.minimal.toolchain
    docker docker-compose
    git
    openssl pkg-config
    sqlx-cli
  ];
}
