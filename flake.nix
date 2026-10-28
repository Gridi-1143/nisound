{
  description = "Nisound - A Linux soundboard";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      system = "x86_64-linux";
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs { inherit system overlays; };

      # for eframe, rodio, symphonia
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
        libpulseaudio
      ];
    in {
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          cmake
          pkg-config
          (rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" ];
          })
        ];

        buildInputs = runtimeLibs;

        shellHook = ''
          export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}:$LD_LIBRARY_PATH"
          echo "Nisound Dev Environment Loaded! Run 'cargo run' to start."
        '';
      };

      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "nisound";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        
        nativeBuildInputs = with pkgs; [ 
          cmake
          pkg-config 
          makeWrapper 
          copyDesktopItems 
        ];
        
        buildInputs = runtimeLibs;

        desktopItems = [
          (pkgs.makeDesktopItem {
            name = "nisound";
            exec = "nisound";
            icon = "nisound";
            desktopName = "Nisound";
            comment = "Linux Soundboard";
            categories = [ "AudioVideo" "Audio" ];
          })
        ];

        postInstall = ''
          install -Dm644 ${./nisound.svg} \
            $out/share/icons/hicolor/scalable/apps/nisound.svg
        '';

        postFixup = ''
          wrapProgram $out/bin/nisound \
            --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibs}" \
            --prefix PATH : "${pkgs.pipewire}/bin"
        '';
      };

      # nix run
      apps.${system}.default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/nisound";
      };
    };
}
