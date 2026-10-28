# Nisound

Nisound is a Linux soundboard with configurable hotkeys and audio routing.

## Requirements

Nisound relies on PipeWire for audio routing. PipeWire must already be enabled and running on your system.

On NixOS, configure PipeWire in `configuration.nix` and rebuild the system.

The Nix package includes `pw-link` from PipeWire in its runtime environment. It does not enable or configure any system services.

For media downloads, install these tools separately:

- `yt-dlp` for downloading audio/video
- - `mutagen` for thumbnail metadata via yt-dlp

## Run directly

run it directly without installing:

```bash
nix run github:Gridi-1143/nisound
```

## Installation

### Recommended: Nix with Cachix

This is the fastest and simplest option for most users.

Enable the binary cache:

```bash
cachix use nisound
```

Then install it into your user profile:

```bash
nix profile install github:Gridi-1143/nisound
```

### NixOS flake

If you want to configure the cache manually instead of using `cachix use nisound`, add this to your Nix configuration:

```nix
nixConfig = {
  extra-substituters = [
    "https://nisound.cachix.org"
  ];
  extra-trusted-public-keys = [
    "nisound.cachix.org-1:xRecNp+wCf9FsSSGEGJD9JpWkN1BUHMI7RjYVdEQ97I="
  ];
};
```

add the input:

```nix
inputs.nisound.url = "github:Gridi-1143/nisound/release";
```

Then add the package to your system configuration:

```nix
environment.systemPackages = [
  inputs.nisound.packages.${pkgs.system}.default
];
```

### AppImage

If you prefer not to use Nix, download the AppImage from the GitHub Releases page, make it executable, and run it:

```bash
chmod +x Nisound*.AppImage
./Nisound*.AppImage
```

This still requires a running PipeWire installation with `pw-link` available.

### Build locally / compile from source

Use this if you do not want to use Cachix or want to build the package yourself.

```bash
git clone https://github.com/Gridi-1143/nisound.git
cd nisound
nix profile install .
```

Or run it directly from the source tree:

```bash
nix run .
```
