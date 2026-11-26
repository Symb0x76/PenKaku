#!/bin/sh
# Install penkaku for the current user (or PREFIX=/usr/local sudo ./install.sh for everyone):
# the binaries, the launcher entry and its icons. Run from the unpacked penkaku folder.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}

install -Dm755 "$here/penkaku" "$prefix/bin/penkaku"
install -Dm755 "$here/penkaku-cli" "$prefix/bin/penkaku-cli"
for png in "$here"/icons/*.png; do
    n=$(basename "$png" .png)
    install -Dm644 "$png" "$prefix/share/icons/hicolor/${n}x${n}/apps/penkaku.png"
done
mkdir -p "$prefix/share/applications"
sed "s|^Exec=.*|Exec=$prefix/bin/penkaku|" "$here/penkaku.desktop" > "$prefix/share/applications/penkaku.desktop"

# Refresh the caches where the tools exist; launchers pick the entry up either way after re-login.
gtk-update-icon-cache -q "$prefix/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$prefix/share/applications" 2>/dev/null || true
echo "Installed to $prefix (penkaku is in the application menu)."
