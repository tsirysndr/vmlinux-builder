{
  description = "Dev environment with Linux build dependencies";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:

    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
      };
    in {
      devShells.${system}.default = pkgs.mkShell {
        buildInputs = with pkgs; [
          deno
          curl
          git
          gcc
          flex
          bison
          ncurses.dev
          openssl.dev
          bc
          elfutils.dev
          pahole
          pkg-config
          perl
        ];
      };
    };
}
