#!/usr/bin/env bash
# Build Brewed Texture Designer for macOS: an app, in a disk image.
#
#   packaging/macos/build.sh                     # → target/dist/Brewed-Texture-Designer-<version>-macos-arm64.dmg
#   packaging/macos/build.sh --universal         # Apple Silicon and Intel in one app
#
# Needs Xcode's command line tools (xcode-select --install) and Rust (https://rustup.rs); for
# --universal, `rustup target add x86_64-apple-darwin` too. The app is signed ad hoc (Apple
# Silicon runs nothing unsigned), not by a developer of Apple's: opened the first time, macOS
# asks (Control-click → Open), or after `xattr -dr com.apple.quarantine "/Applications/Brewed Texture Designer.app"`.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
name="Brewed Texture Designer"
bin="brewed-texture-designer"

if [ "${1:-}" = "--universal" ]; then
    cargo build --release --locked -p "$bin" --target aarch64-apple-darwin
    cargo build --release --locked -p "$bin" --target x86_64-apple-darwin
    mkdir -p target/universal
    lipo -create -output "target/universal/$bin" "target/aarch64-apple-darwin/release/$bin" "target/x86_64-apple-darwin/release/$bin"
    built="target/universal/$bin"
    arch="universal"
else
    cargo build --release --locked -p "$bin" --target aarch64-apple-darwin
    built="target/aarch64-apple-darwin/release/$bin"
    arch="arm64"
fi

# The app: the program, what it is, its icon.
app="target/macos/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$built" "$app/Contents/MacOS/$bin"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$name</string>
    <key>CFBundleDisplayName</key><string>$name</string>
    <key>CFBundleIdentifier</key><string>io.github.wtfmydarling.BrewedTextureDesigner</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleShortVersionString</key><string>${version%%-*}</string>
    <key>CFBundleExecutable</key><string>$bin</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>Brewed Texture Designer package</string>
            <key>CFBundleTypeExtensions</key><array><string>brewtex</string></array>
            <key>CFBundleTypeRole</key><string>Editor</string>
        </dict>
    </array>
</dict>
</plist>
EOF

# The icon, of the logo: drawn by Quick Look, made every size an icon has.
icons="target/macos/AppIcon.iconset"
rm -rf "$icons"
mkdir -p "$icons"
qlmanage -t -s 1024 -o target/macos crates/brewed-texture-designer/assets/logo.svg >/dev/null
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" target/macos/logo.svg.png --out "$icons/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) target/macos/logo.svg.png --out "$icons/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$icons" -o "$app/Contents/Resources/AppIcon.icns"

codesign --force --deep --sign - "$app"

# The disk image: the app, and Applications to drag it to.
stage="target/macos/dmg"
rm -rf "$stage"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
mkdir -p target/dist
out="target/dist/Brewed-Texture-Designer-$version-macos-$arch.dmg"
rm -f "$out"
hdiutil create -volname "$name" -srcfolder "$stage" -ov -format UDZO "$out" >/dev/null
echo "Built $out"
