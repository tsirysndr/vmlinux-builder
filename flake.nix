{
  description = "Linux kernel builder and development environment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    crane = {
      url = "github:ipetkov/crane";
    };
    bsdkrun = {
      url = "github:tsirysndr/bsdkrun";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, crane, bsdkrun, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = function:
        builtins.listToAttrs (map (system: {
          name = system;
          value = function system;
        }) systems);
    in {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          craneLib = crane.mkLib pkgs;
          commonArgs = {
            pname = "kbuildx";
            version = "0.1.0";
            src = craneLib.cleanCargoSource ./kbuildx;
            strictDeps = true;
            nativeBuildInputs = [ pkgs.makeWrapper ];
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          kbuildx = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            postInstall = ''
              wrapProgram $out/bin/kbuildx \
                --prefix PATH : ${pkgs.lib.makeBinPath [ bsdkrun.packages.${system}.default ]}
            '';
          });
        in {
          inherit kbuildx;
          default = kbuildx;
        });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell {
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
        });
    };
}
