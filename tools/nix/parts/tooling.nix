_: {
  perSystem = {
    config,
    lib,
    pkgs,
    system,
    ...
  }: {
    devShells.tooling = pkgs.mkShell {
      packages = with pkgs; [
        config.packages.bats
        commitlint
        curl
        git
        gradle_7
        graphviz
        jq
      ];

      BATS_LIB = "${config.packages.bats}/share/bats";
    };

    # Responsible only for formatting this flake itself
    formatter = pkgs.alejandra;

    packages.bats = pkgs.bats.withLibraries (p: [p.bats-assert p.bats-file p.bats-support]);
  };
}
