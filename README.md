# md-viewer

A lightweight markdown viewer built with Rust, egui, and egui_commonmark.

## Screenshots

| Dark Mode | Light Mode |
|:---------:|:----------:|
| ![Dark Mode](screenshots/dark-mode.png) | ![Light Mode](screenshots/light-mode.png) |

## Features

- Fast 60 FPS rendering
- GitHub Flavored Markdown support
- Syntax highlighting for 200+ languages
- Live file reload
- Dark/Light theme
- Native file dialogs
- Drag and drop support
- Cross-platform (Linux X11/Wayland)

## Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release
```

## Usage

```bash
# Open a file
./target/release/md-viewer README.md

# Open with live reload
./target/release/md-viewer README.md --watch
```

## System Dependencies (Arch Linux)

```bash
sudo pacman -S --needed \
    base-devel clang pkg-config \
    libxcb libxkbcommon openssl \
    gtk3 fontconfig dbus zenity \
    xdg-desktop-portal xdg-desktop-portal-gtk
```

## License

MIT
