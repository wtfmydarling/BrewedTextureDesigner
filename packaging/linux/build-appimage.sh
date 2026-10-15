#!/usr/bin/env bash
# Build Brewed Texture Designer as an AppImage: one file that runs on most Linux systems.
#
#   packaging/linux/build-appimage.sh            # → target/dist/Brewed_Texture_Designer-<version>-x86_64.AppImage
#
# Build it on the oldest system it is to run on (the binary needs the glibc it was built with, or
# newer): Debian 13 is a good one. Needs a C compiler and Rust (https://rustup.rs); the binary
# loads Wayland, X11 and Vulkan itself at run time, so nothing else is bundled. appimagetool is
# taken from PATH, or from target/tools, where it is downloaded the first time (from the AppImage
# project's releases on GitHub).
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
id="io.github.wtfmydarling.BrewedTextureDesigner"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
arch="$(uname -m)"

for tool in cargo cc; do
    if ! command -v "$tool" >/dev/null; then
        echo "Missing: $tool." >&2
        echo "  Debian/Ubuntu: sudo apt install build-essential curl  (then Rust: https://rustup.rs)" >&2
        echo "  Fedora:        sudo dnf install gcc curl              (then Rust: https://rustup.rs)" >&2
        exit 1
    fi
done

cargo build --release --locked -p brewed-texture-designer

# The AppDir: the program, its desktop file, its icon, what it opens, and what starts it.
app="target/appimage/BrewedTextureDesigner.AppDir"
rm -rf "$app"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/scalable/apps" "$app/usr/share/metainfo" "$app/usr/share/mime/packages"
install -m 755 target/release/brewed-texture-designer "$app/usr/bin/"
install -m 644 "packaging/linux/$id.desktop" "$app/usr/share/applications/"
install -m 644 "packaging/linux/$id.metainfo.xml" "$app/usr/share/metainfo/$id.appdata.xml"
install -m 644 "packaging/linux/$id.mime.xml" "$app/usr/share/mime/packages/$id.xml"
install -m 644 crates/brewed-texture-designer/assets/logo.svg "$app/usr/share/icons/hicolor/scalable/apps/$id.svg"
cp "packaging/linux/$id.desktop" "$app/"
cp crates/brewed-texture-designer/assets/logo.svg "$app/$id.svg"
cat > "$app/AppRun" <<'EOF'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/brewed-texture-designer" "$@"
EOF
chmod 755 "$app/AppRun"

# appimagetool: of the system, or downloaded once.
tool="$(command -v appimagetool || true)"
if [ -z "$tool" ]; then
    tool="target/tools/appimagetool-$arch.AppImage"
    if [ ! -x "$tool" ]; then
        mkdir -p target/tools
        echo "Downloading appimagetool…"
        curl -fL -o "$tool" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$arch.AppImage"
        chmod 755 "$tool"
    fi
fi

mkdir -p target/dist
out="target/dist/Brewed_Texture_Designer-$version-$arch.AppImage"
# (Without FUSE, as in a container or WSL, the tool runs extracted.)
ARCH="$arch" APPIMAGE_EXTRACT_AND_RUN=1 "$tool" "$app" "$out"
echo "Built $out"
