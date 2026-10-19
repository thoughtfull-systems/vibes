# SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

{
  description = "Personal, local-first apps for Linux, Android, and the web";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, self }:
    let
      commonplace = ./commonplace/linux/package.nix;
      forAllSystems = nixpkgs.lib.genAttrs [
        # keep-sorted start
        "aarch64-linux"
        "x86_64-linux"
        # keep-sorted end
      ];
    in
    {
      # keep-sorted start block=yes newline_separated=yes
      overlays.default = final: _: {
        commonplace = final.callPackage commonplace { };
      };

      packages = forAllSystems (system: {
        # keep-sorted start block=yes
        commonplace = nixpkgs.legacyPackages.${system}.callPackage commonplace { };
        default = self.packages.${system}.commonplace;
        # keep-sorted end
      });
      # keep-sorted end
    };
}
