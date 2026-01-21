.PHONY: build release install run clean

build:
	cargo build

release:
	cargo build --release

install: release
	cp target/release/md-viewer ~/.local/bin/

run:
	cargo run

clean:
	cargo clean
