# md-viewer

[![Crates.io](https://img.shields.io/crates/v/md-viewer.svg)](https://crates.io/crates/md-viewer)
[![AUR](https://img.shields.io/aur/version/md-viewer-git)](https://aur.archlinux.org/packages/md-viewer-git)
[![Snap](https://img.shields.io/badge/snap-md--viewer-blue?logo=snapcraft)](https://snapcraft.io/md-viewer)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![GitHub stars](https://img.shields.io/github/stars/aydiler/md-viewer)](https://github.com/aydiler/md-viewer/stargazers)

A fast, lightweight markdown viewer for Linux built with Rust and egui. Designed for distraction-free reading with excellent typography and syntax highlighting.

![Dark Mode](screenshots/dark-mode.png)

## Features

### Rendering
- **GitHub Flavored Markdown** - Full GFM support including tables, task lists, and footnotes
- **Syntax Highlighting** - 200+ languages via syntect with beautiful color schemes
- **Mermaid Diagrams** - Flowcharts, sequence diagrams, and more rendered natively via [merman](https://github.com/Latias94/merman) (click to enlarge)
- **HTML Tables** - Rendered as formatted grids with proper cell padding
- **Images & SVG** - Embedded and remote image support (PNG, JPEG, GIF, SVG, HTTP URLs)
- **Unicode Support** - System font fallbacks (Noto, DejaVu) for emojis, CJK, and non-Latin scripts
- **60 FPS Rendering** - Smooth scrolling with viewport-based lazy rendering
- **Typography** - 1.5x line height for optimal readability (WCAG 2.1 compliant)

### Navigation
- **Tab System** - Open multiple documents with tab bar (Ctrl+Click links to open in new tab)
- **File Explorer** - Hierarchical sidebar with lazy-loading directories and sorting options
- **Outline Sidebar** - Click-to-navigate table of contents from document headers
- **Navigation Buttons** - Back/forward buttons in title bar for quick history navigation
- **Per-Tab History** - Independent back/forward navigation within each tab (Alt+Left/Right)
- **Internal Links** - Navigate between markdown files with relative links

### View
- **Dark & Light Themes** - Toggle with Ctrl+D
- **Zoom** - 50% to 300% zoom (Ctrl++/-/0 or Ctrl+Scroll)
- **Live Reload** - Auto-refresh on file changes (enabled by default)

### Usability
- **Drag and Drop** - Drop markdown files onto the window to open
- **Native Dialogs** - System file picker integration
- **Session Persistence** - Remembers open tabs, theme, zoom, and sidebar state
- **Cross-Platform** - Works on X11 and Wayland

## Screenshots

### Dark Mode
*Mermaid flowchart, tech stack table, file explorer, and outline sidebar*

![Dark Mode](screenshots/dark-mode.png)

### Syntax Highlighting
*Rust and YAML code blocks with semantic coloring, sequence diagram below*

![Syntax Highlighting](screenshots/syntax-highlighting.png)

### Light Mode
*Same features in clean light theme -- mermaid diagram and table rendering*

![Light Mode](screenshots/light-mode.png)

### Sequence Diagrams & Blockquotes
*Mermaid sequence diagram, blockquote, and bullet list rendering*

![Tables](screenshots/tables.png)

## Keyboard Shortcuts

### Tab Management

| Shortcut | Action |
|----------|--------|
| Ctrl+T | New tab (open file dialog) |
| Ctrl+W | Close current tab |
| Ctrl+Tab | Next tab |
| Ctrl+Shift+Tab | Previous tab |
| Ctrl+1-9 | Switch to tab 1-9 |

### Navigation

| Shortcut | Action |
|----------|--------|
| Ctrl+O | Open file dialog |
| Alt+Left | Navigate back in history |
| Alt+Right | Navigate forward in history |
| Click link | Navigate in current tab |
| Ctrl+Click link | Open link in new tab |

### View

| Shortcut | Action |
|----------|--------|
| Ctrl+D | Toggle dark/light mode |
| Ctrl+Shift+E | Toggle file explorer |
| Ctrl+Shift+O | Toggle outline sidebar |
| Ctrl++ / Ctrl+= | Zoom in |
| Ctrl+- | Zoom out |
| Ctrl+0 | Reset zoom to 100% |
| Ctrl+Scroll | Zoom with mouse wheel |

### File Operations

| Shortcut | Action |
|----------|--------|
| F5 | Toggle file watching |
| Ctrl+Q | Quit application |

## Installation

### Arch Linux (AUR)

```bash
# Using yay
yay -S md-viewer-git

# Using paru
paru -S md-viewer-git
```

### Snap Store

```bash
sudo snap install md-viewer
```

### Cargo (crates.io)

```bash
cargo install md-viewer
```

### From Source

```bash
# Clone the repository
git clone https://github.com/aydiler/md-viewer
cd md-viewer

# Build release binary
cargo build --release

# Install to ~/.local/bin (optional)
make install
```

### System Dependencies (Arch Linux)

```bash
sudo pacman -S --needed \
    base-devel clang pkg-config \
    libxcb libxkbcommon openssl \
    gtk3 fontconfig dbus zenity \
    xdg-desktop-portal xdg-desktop-portal-gtk
```

## Usage

```bash
# Open a file (live reload is enabled by default)
md-viewer README.md

# Disable live reload
md-viewer README.md --no-watch
```

## Technical Details

- **Binary size**: ~35 MB (includes syntax highlighting, mermaid renderer, math rendering, image support, X11+Wayland). ~7 MB as snap.
- **Startup time**: < 200ms
- **Rendering**: 60 FPS with viewport-based clipping
- **Memory**: Uses mimalloc for improved allocation performance
- **Platform**: Linux (X11 and Wayland via glow backend)

### Built With

- [eframe/egui](https://github.com/emilk/egui) - Immediate mode GUI framework
- [egui_commonmark](https://github.com/lampsitter/egui_commonmark) - Markdown rendering (vendored fork with typography improvements)
- [merman](https://github.com/Latias94/merman) - Mermaid diagram rendering
- [syntect](https://github.com/trishume/syntect) - Syntax highlighting
- [notify](https://github.com/notify-rs/notify) - File watching
- [rfd](https://github.com/PolyMeilex/rfd) - Native file dialogs

## License

MIT
