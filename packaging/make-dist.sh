#!/bin/sh
# Build a binary release tarball: packaging/make-dist.sh 0.1.0
set -e
V=${1:?usage: make-dist.sh VERSION}
ARCH=$(uname -m)
cd "$(dirname "$0")/.."
cargo build --release --workspace
D=dist/subs2srs-$V-$ARCH-linux
rm -rf "$D" && mkdir -p "$D"
install -m755 target/release/subs2srs target/release/subs2srs-cli packaging/install.sh "$D/"
install -Dm644 data/io.github.subs2srs.Subs2srs.desktop "$D/share/applications/io.github.subs2srs.Subs2srs.desktop"
install -Dm644 data/io.github.subs2srs.Subs2srs.metainfo.xml "$D/share/metainfo/io.github.subs2srs.Subs2srs.metainfo.xml"
install -Dm644 data/icons/app/io.github.subs2srs.Subs2srs.svg "$D/share/icons/hicolor/scalable/apps/io.github.subs2srs.Subs2srs.svg"
cp README.md LICENSE CHANGELOG.md "$D/"
tar -C dist -czf "$D.tar.gz" "subs2srs-$V-$ARCH-linux"
(cd dist && sha256sum "subs2srs-$V-$ARCH-linux.tar.gz" > "subs2srs-$V-$ARCH-linux.tar.gz.sha256")
echo "$D.tar.gz"
