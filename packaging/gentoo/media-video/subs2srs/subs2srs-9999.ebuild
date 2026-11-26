# Copyright 2026
# Distributed under the terms of the GNU General Public License v3

EAPI=8

inherit cargo desktop git-r3 xdg

DESCRIPTION="Create Anki flashcards from videos and subtitles (Rust/GTK4 port of subs2srs)"
HOMEPAGE="https://github.com/Ajatt-Tools/subs2srs"
EGIT_REPO_URI="https://github.com/tomjfinch/subs2srs-linux.git"

LICENSE="GPL-3+"
# Dependent crate licenses
LICENSE+=" Apache-2.0 BSD MIT Unicode-3.0 || ( Apache-2.0 MIT )"
SLOT="0"
KEYWORDS=""

DEPEND="
	>=gui-libs/gtk-4.12:4
	dev-libs/glib:2
"
RDEPEND="${DEPEND}
	media-video/ffmpeg[mp3,opus,x264]
	gnome-base/librsvg
"
BDEPEND="virtual/pkgconfig"

QA_FLAGS_IGNORED="usr/bin/.*"

src_unpack() {
	git-r3_src_unpack
	cargo_live_src_unpack
}

src_install() {
	dobin "$(cargo_target_dir)"/subs2srs "$(cargo_target_dir)"/subs2srs-cli
	domenu data/io.github.subs2srs.Subs2srs.desktop
	insinto /usr/share/metainfo
	doins data/io.github.subs2srs.Subs2srs.metainfo.xml
	insinto /usr/share/icons/hicolor/scalable/apps
	doins data/icons/app/io.github.subs2srs.Subs2srs.svg
	einstalldocs
}
