{ inputs, ... }:
{
  flake.overlays.default =
    final: _:
    let
      mkZode = import ../toolchain.nix { inherit inputs; };
    in
    {
      zode-editor = mkZode final;
    };
}
