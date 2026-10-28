# Nisound

Nisound is a Linux soundboard with configurable hotkeys and audio routing.

## NixOS

Run directly from GitHub:

```bash
nix run github:Gridi-1143/nisound
```

Install it into the user profile:

```bash
nix profile install github:Gridi-1143/nisound
```

To use the binary cache, install Cachix and enable the `nisound` cache before
running the commands above:

```bash
cachix use nisound
```

For a NixOS flake, add the input:

```nix
inputs.nisound.url = "github:Gridi-1143/nisound";
```

Then add the package to your system configuration:

```nix
environment.systemPackages = [
  inputs.nisound.packages.${pkgs.system}.default
];
```

This requires passing `inputs.nisound` and `pkgs` in the scope where the
system packages are declared.

The Nix package includes the `pw-link` command from PipeWire in its runtime
environment. It does not enable or configure any system services.

For audio routing, PipeWire **must already be enabled and running on the system**.
On NixOS, configure PipeWire in `configuration.nix` and rebuild the system.

## AppImage

Download the AppImage from the GitHub Releases page, make it executable, and
run it:

```bash
chmod +x Nisound*.AppImage
./Nisound*.AppImage
```

The AppImage does not modify the system. Audio routing still requires a
running PipeWire installation with `pw-link` available.
