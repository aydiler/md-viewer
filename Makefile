.PHONY: build release install run clean

build:
	cargo build

release:
	cargo build --release

install: release
	cp target/release/md-viewer ~/.local/bin/
	mkdir -p ~/.local/share/applications
	cp data/md-viewer.desktop ~/.local/share/applications/
	update-desktop-database ~/.local/share/applications 2>/dev/null || true

run:
	cargo run

clean:
	cargo clean
