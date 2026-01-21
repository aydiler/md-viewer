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
- Zoom support (50% - 300%)
- Left sidebar outline with click-to-navigate
- Native file dialogs
- Drag and drop support
- Cross-platform (Linux X11/Wayland)

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+O | Open file |
| Ctrl+W | Toggle live reload |
| Ctrl+D | Toggle dark/light mode |
| Ctrl+Shift+O | Toggle outline sidebar |
| Ctrl+Q | Quit |
| Ctrl++ / Ctrl+= | Zoom in |
| Ctrl+- | Zoom out |
| Ctrl+0 | Reset zoom to 100% |
| Ctrl+Scroll | Zoom with mouse wheel |

## Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Build and install to ~/.local/bin
make install
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
