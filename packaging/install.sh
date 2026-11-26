#!/bin/sh
# Install subs2srs. Usage: ./install.sh [PREFIX]   (default /usr/local; use ~/.local for a user install)
set -e
PREFIX=${1:-/usr/local}
cd "$(dirname "$0")"
install -Dm755 subs2srs "$PREFIX/bin/subs2srs"
install -Dm755 subs2srs-cli "$PREFIX/bin/subs2srs-cli"
install -Dm644 share/applications/io.github.subs2srs.Subs2srs.desktop "$PREFIX/share/applications/io.github.subs2srs.Subs2srs.desktop"
install -Dm644 share/metainfo/io.github.subs2srs.Subs2srs.metainfo.xml "$PREFIX/share/metainfo/io.github.subs2srs.Subs2srs.metainfo.xml"
install -Dm644 share/icons/hicolor/scalable/apps/io.github.subs2srs.Subs2srs.svg "$PREFIX/share/icons/hicolor/scalable/apps/io.github.subs2srs.Subs2srs.svg"
gtk-update-icon-cache -q -t -f "$PREFIX/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
echo "Installed to $PREFIX. Run: subs2srs"
