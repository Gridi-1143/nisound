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
nix run github:Gridi-1143/nisound/release
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
nix profile install github:Gridi-1143/nisound/release
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

The `release` branch points to the latest stable release successfully uploaded to
Cachix. The binary package currently supports `x86_64-linux`. Keep Nisound's own
locked dependencies: overriding its inputs (for example with `nixpkgs.follows`)
can produce a different package that is not in the cache.

### Updating

You do not need to change the input URL for new releases. Flakes pin a revision in
`flake.lock`, so update your NixOS configuration's lock file and rebuild:

```bash
nix flake update nisound
sudo nixos-rebuild switch --flake .#hostname
```

Replace `hostname` with your NixOS configuration name. A regular `nix flake update`
also updates Nisound. Unattended updates require an update schedule configured on
your system; the application does not update itself.

For a user-profile installation, use `nix profile list` to find its installed name,
then run `nix profile upgrade <name>`.

### Publishing releases

Publish a stable GitHub release from `main` with a committed `flake.lock`. The Nix
workflow builds its tag, uploads the package and runtime dependencies to Cachix,
and only then advances `release` to that exact commit. Pushes to `main` do not
trigger a build. Failed builds/uploads leave the channel unchanged, and
prereleases do not enter the stable channel.

To retry or initialize the channel for an existing release, run **Build Nix
package** from the Actions tab on `main` and provide the release tag. Only the
release designated latest by GitHub can advance the channel. Promotion uses a
fast-forward push, so stable release commits must descend from the current
`release` commit. Repository rules must allow GitHub Actions to update `release`.

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
