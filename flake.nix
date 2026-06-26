{
  description = "Dev env for Nisound for NixOS";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      system = "x86_64-linux";
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs { inherit system overlays; };

      # for eframe/winit, rodio
      runtimeLibs = with pkgs; [
        alsa-lib
        libxkbcommon
        wayland
        vulkan-loader
        libGL
        libX11
        libXcursor
        libXi
        libXrandr
        libXrender
        libXext
        libXinerama
        libXtst
      ];
    in {
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          pkg-config
          (rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" ];
          })
        ];

        buildInputs = runtimeLibs;

        shellHook = ''
          export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}:$LD_LIBRARY_PATH"
          echo " Nisound Dev Environment Loaded! Run 'cargo run' to start."
        '';
      };

      # basic nix build
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "nisound";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = runtimeLibs;

        postFixup = ''
          patchelf --set-rpath "${pkgs.lib.makeLibraryPath runtimeLibs}" $out/bin/nisound
        '';
      };
    };
}
