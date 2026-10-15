#!/usr/bin/env bash
# Build Brewed Texture Designer as a Flatpak bundle, from this checkout.
#
#   packaging/linux/build-flatpak.sh             # → target/dist/BrewedTextureDesigner-<version>.flatpak
#   flatpak install --user target/dist/BrewedTextureDesigner-<version>.flatpak
#
# Needs flatpak and flatpak-builder (Fedora: sudo dnf install flatpak flatpak-builder; Debian:
# sudo apt install flatpak flatpak-builder) and Flathub as a remote of the user's, for the
# runtime, the SDK and its Rust; they are installed here the first time.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
id="io.github.wtfmydarling.BrewedTextureDesigner"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"

for tool in flatpak flatpak-builder; do
    if ! command -v "$tool" >/dev/null; then
        echo "Missing: $tool (Fedora: sudo dnf install flatpak flatpak-builder; Debian: sudo apt install flatpak flatpak-builder)." >&2
        exit 1
    fi
done
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo

flatpak-builder --user --install-deps-from=flathub --force-clean --repo=target/flatpak/repo target/flatpak/build "packaging/linux/$id.yml"
mkdir -p target/dist
out="target/dist/BrewedTextureDesigner-$version.flatpak"
flatpak build-bundle target/flatpak/repo "$out" "$id"
echo "Built $out (install: flatpak install --user $out)"
