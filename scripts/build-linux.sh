#!/usr/bin/env bash
# Builds the Linux bundles (AppImage and .deb) on Debian/Ubuntu, natively or in a WSL distro.
#
#   From Windows:  wsl -d Ubuntu --exec bash scripts/build-linux.sh
#   On Linux:      bash scripts/build-linux.sh
#
# The project is copied (files Git tracks or would track, without node_modules or target) to
# ~/.cache/in8-build, so Windows and Linux never share node_modules. The bundles end up in
# src-tauri/target/linux-bundle/. An AppImage only runs on distros with this glibc or newer.
set -euo pipefail

src="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="${IN8_BUILD_DIR:-$HOME/.cache/in8-build}"

needed=(build-essential pkg-config file npm libwebkit2gtk-4.1-dev libxdo-dev libssl-dev
    libayatana-appindicator3-dev librsvg2-dev)
missing=()
for pkg in "${needed[@]}"; do
    dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null | grep -q "install ok installed" || missing+=("$pkg")
done
if ((${#missing[@]})); then
    echo "Missing packages. Install them with:"
    echo "  sudo apt install ${missing[*]}"
    exit 1
fi
if ! command -v cargo >/dev/null; then
    echo "Rust isn't installed. Install rustup and run: rustup default stable"
    exit 1
fi

mkdir -p "$work"
git -c safe.directory="$src" -C "$src" ls-files -z --cached --others --exclude-standard |
    tar -C "$src" --null -T - -cf - | tar -C "$work" -xf -

cd "$work"
npm ci
# linuxdeploy is an AppImage too; extracting it instead of mounting avoids needing FUSE (e.g. in WSL).
export APPIMAGE_EXTRACT_AND_RUN=1
npx tauri build --bundles appimage,deb

out="$src/src-tauri/target/linux-bundle"
mkdir -p "$out"
cp src-tauri/target/release/bundle/appimage/*.AppImage src-tauri/target/release/bundle/deb/*.deb "$out/"
echo "Bundles copied to $out"
