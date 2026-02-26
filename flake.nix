{
  description = "AI-powered merge conflict resolution using Claude Code";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
      ];

      forAllSystems =
        function:
        builtins.listToAttrs (
          builtins.map (system: {
            name = system;
            value = function system;
          }) systems
        );

      makePkgs =
        localSystem:
        import nixpkgs {
          inherit localSystem;
          overlays = [ self.overlays.default ];
        };

    in

    {

      packages = forAllSystems (
        system:
        let
          pkgs = makePkgs system;
        in
        {
          inherit pkgs;
          inherit (pkgs) claude-mergetool;
          default = pkgs.claude-mergetool;
        }
      );

      overlays.default = import ./nix/overlays/claude-mergetool.nix;

    };
}
