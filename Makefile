.PHONY: build release install uninstall run clean

build:
	cargo build

release:
	cargo build --release

install: release
	cp target/release/md-viewer ~/.local/bin/
	mkdir -p ~/.local/share/applications
	cp data/md-viewer.desktop ~/.local/share/applications/
	mkdir -p ~/.local/share/licenses/md-viewer
	cp LICENSE THIRD_PARTY_NOTICES ~/.local/share/licenses/md-viewer/
	update-desktop-database ~/.local/share/applications 2>/dev/null || true

uninstall:
	rm -f ~/.local/bin/md-viewer
	rm -f ~/.local/share/applications/md-viewer.desktop
	rm -rf ~/.local/share/licenses/md-viewer
	update-desktop-database ~/.local/share/applications 2>/dev/null || true

run:
	cargo run

clean:
	cargo clean
