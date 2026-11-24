{
  description = "Read-only Reddit research CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        reddit-cli = pkgs.callPackage ./nix/reddit-cli.nix { };
      in
      {
        packages = {
          inherit reddit-cli;
          default = reddit-cli;
        };
        checks.reddit-cli = reddit-cli;
      }
    );
}
