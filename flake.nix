{
  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    systems.url = "github:nix-systems/default";
    devenv = {
      url = "github:cachix/devenv";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  outputs =
    {
      self,
      nixpkgs,
      devenv,
      systems,
      ...
    }@inputs:
    let
      forEachSystem = nixpkgs.lib.genAttrs (import systems);
    in
    {
      packages = forEachSystem (system: {
        devenv-up = self.devShells.${system}.default.config.procfileScript;
        devenv-test = self.devShells.${system}.default.config.test;
      });

      devShells = forEachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = devenv.lib.mkShell {
            inherit inputs pkgs;
            modules = [
              {
                packages = with pkgs; [
                  bun

                  # As recommended by https://v2.tauri.app/start/prerequisites/
                  at-spi2-atk
                  atkmm
                  cairo
                  cargo-tauri
                  gdk-pixbuf
                  glib
                  gobject-introspection
                  gtk3
                  harfbuzz
                  librsvg
                  libsoup_3
                  libuuid
                  openssl
                  pango
                  pkg-config
                  pkg-config
                  webkitgtk_4_1
                  xdg-utils
                ];

                # https://devenv.sh/languages/
                languages.rust = {
                  enable = true;
                  channel = "nightly";
                };

                # https://devenv.sh/scripts/
                scripts = {
                  build.exec = ''
                    ${pkgs.bun}/bin/bun tauri build
                  '';
                  dev.exec = ''
                    ${pkgs.bun}/bin/bun tauri dev
                  '';
                };

                enterShell = ''
                  echo
                  echo Bun version: ''$(${pkgs.bun}/bin/bun --version)
                  echo Cargo version: ''$(cargo --version)
                  echo Rust version: ''$(rustc --version)
                  echo
                '';

                env.LD_LIBRARY_PATH = "${nixpkgs.lib.makeLibraryPath [ pkgs.libuuid ]}:$LD_LIBRARY_PATH";

                # https://devenv.sh/tasks/
                tasks = {
                  "koharu:setup".exec = ''
                    ${pkgs.bun}/bin/bun install
                    if [ ! -e ".env" ]; then
                      cp .env.example .env
                    fi
                  '';
                  "devenv:enterShell".after = [ "koharu:setup" ];
                };

                # https://devenv.sh/tests/
                enterTest = ''
                  echo "Running tests"
                  cargo test
                '';

                # https://devenv.sh/git-hooks/
                git-hooks.hooks = {
                  clippy.enable = true;
                  clippy.settings.allFeatures = true;
                  clippy.settings.offline = false;
                  eslint.enable = true;
                  ripsecrets.enable = true;
                  ripsecrets.excludes = [ ".env.example" ];
                };

                # loads any existing dotenv file
                dotenv.enable = true;
              }
            ];
          };
        }
      );
    };
}
