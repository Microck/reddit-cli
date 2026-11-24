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
        reddit-research = pkgs.callPackage ./nix/reddit-research.nix { };
      in
      {
        packages = {
          inherit reddit-research;
          default = reddit-research;
        };
        checks.reddit-research = reddit-research;
      }
    );
}
