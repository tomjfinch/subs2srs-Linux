PREFIX ?= /usr/local
DESTDIR ?=
BINDIR = $(DESTDIR)$(PREFIX)/bin
DATADIR = $(DESTDIR)$(PREFIX)/share
APP_ID = io.github.subs2srs.Subs2srs
CARGO ?= cargo

.PHONY: all build test install uninstall clean run

all: build

build:
	$(CARGO) build --release --workspace

test:
	$(CARGO) test --workspace

run:
	$(CARGO) run

install:
	install -Dm755 target/release/subs2srs $(BINDIR)/subs2srs
	install -Dm755 target/release/subs2srs-cli $(BINDIR)/subs2srs-cli
	install -Dm644 data/$(APP_ID).desktop $(DATADIR)/applications/$(APP_ID).desktop
	install -Dm644 data/$(APP_ID).metainfo.xml $(DATADIR)/metainfo/$(APP_ID).metainfo.xml
	install -Dm644 data/icons/app/$(APP_ID).svg $(DATADIR)/icons/hicolor/scalable/apps/$(APP_ID).svg
	-gtk-update-icon-cache -q -t -f $(DATADIR)/icons/hicolor 2>/dev/null || true
	-update-desktop-database -q $(DATADIR)/applications 2>/dev/null || true

uninstall:
	rm -f $(BINDIR)/subs2srs $(BINDIR)/subs2srs-cli
	rm -f $(DATADIR)/applications/$(APP_ID).desktop
	rm -f $(DATADIR)/metainfo/$(APP_ID).metainfo.xml
	rm -f $(DATADIR)/icons/hicolor/scalable/apps/$(APP_ID).svg

clean:
	$(CARGO) clean
