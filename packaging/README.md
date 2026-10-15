# Packaging

How Brewed Texture Designer is built for each system, by the scripts beside this file. Each
writes what it makes to `target/dist`, named by the version in `Cargo.toml`. Native packages of
Linux distributions (`.deb`, `.rpm`…) are not made: a program is given as an AppImage or a Flatpak.

| System | Command | Makes | Needs |
|---|---|---|---|
| Windows | `powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1` | an installer (`…-windows-x64-setup.exe`) and a zip | Rust (MSVC); Inno Setup 6 for the installer (`winget install JRSoftware.InnoSetup`) |
| Linux | `packaging/linux/build-appimage.sh` | an AppImage (`Brewed_Texture_Designer-…-x86_64.AppImage`) | Rust, a C compiler; appimagetool (downloaded the first time) |
| Linux | `packaging/linux/build-flatpak.sh` | a Flatpak bundle (`BrewedTextureDesigner-….flatpak`) | flatpak, flatpak-builder (the runtime, SDK and Rust come from Flathub) |
| macOS | `packaging/macos/build.sh` (`--universal` for Intel too) | a disk image of the app (`…-macos-arm64.dmg`) | Xcode's command line tools, Rust |

- **Windows**: installed for the user alone, or for everyone where chosen; a Start menu entry,
  a desktop one where asked; `.brewtex` packages open in it; uninstalled from Settings → Apps.
- **Linux**: the AppImage is built on the oldest system it is to run on (it needs the glibc it
  was built with, or newer): Debian 13 is a good one. Nothing is bundled but the program: it loads
  Wayland, X11 and Vulkan of the system itself. The Flatpak is built from the source in the
  Freedesktop SDK (25.08), fetching the crates as it builds: a local build; for Flathub the
  crates are to be listed for an offline build (flatpak-cargo-generator).
- **macOS**: the app is signed ad hoc, not by a developer of Apple's: the first time it is opened
  with Control-click → Open, or after `xattr -dr com.apple.quarantine "/Applications/Brewed Texture Designer.app"`.

A release on GitHub is made from its tag (Releases → Draft a new release → the tag, "Set as a
pre-release" for a pre-release), the files of `target/dist` attached.
