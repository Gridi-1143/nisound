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
