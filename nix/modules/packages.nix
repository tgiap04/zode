{ inputs, ... }:
{
  perSystem =
    { pkgs, ... }:
    let
      mkZode = import ../toolchain.nix { inherit inputs; };
      zode-editor = mkZode pkgs;
    in
    {
      packages = {
        default = zode-editor;
        debug = zode-editor.override { profile = "dev"; };
      };
    };
}
