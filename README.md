# md-viewer

[![Crates.io](https://img.shields.io/crates/v/md-viewer.svg)](https://crates.io/crates/md-viewer)
[![AUR](https://img.shields.io/aur/version/md-viewer-git)](https://aur.archlinux.org/packages/md-viewer-git)
[![Snap](https://img.shields.io/badge/snap-md--viewer-blue?logo=snapcraft)](https://snapcraft.io/md-viewer)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![GitHub stars](https://img.shields.io/github/stars/aydiler/md-viewer)](https://github.com/aydiler/md-viewer/stargazers)

A fast, lightweight markdown viewer for Linux, with Windows and macOS builds too, written in Rust with egui. Designed for distraction-free reading with excellent typography, syntax highlighting, and LaTeX math — from quick notes to scientific papers.

![md-viewer rendering a document with LaTeX math, a table and a figure](screenshots/math-rendering.png)

## Features

### Rendering
- **GitHub Flavored Markdown** - Full GFM support including tables, task lists, footnotes, and recognized emoji shortcodes such as `:pushpin:`
- **LaTeX Math** - Inline `$…$` and display `$$…$$` equations rendered via typst + mitex — fractions, sub/superscripts, `\boxed`, accents, matrices, and more — sized and baseline-aligned to the surrounding text
- **Syntax Highlighting** - 200+ languages via syntect with beautiful color schemes
- **Mermaid Diagrams** - Flowcharts, sequence diagrams, and more rendered natively via [merman](https://github.com/Latias94/merman) (click to enlarge)
- **Tables that fill the pane** - Columns stretch proportionally to the available width and re-fit whenever the window, the Full Width setting or a sidebar changes size; cell text wraps instead of clipping, and you can drag the dividers to override
- **HTML Tables** - Rendered as formatted grids with proper cell padding
- **YAML Frontmatter** - A leading `---` block renders as a key/value table instead of raw text
- **Images & SVG** - Embedded and remote image support (PNG, JPEG, GIF, SVG, HTTP URLs)
- **Right-to-Left Text** - Paragraphs, headings, list items and table cells in Arabic, Hebrew and other right-to-left scripts are right-aligned, and lines that mix directions read in the correct order (not in `cargo install` builds; see [Cargo](#cargo-cratesio--slower-builds-from-source))
- **Unicode Support** - Installed system fonts are picked per script for CJK and other non-Latin text; a monochrome emoji font is bundled and a color one is used when installed
- **Viewport Rendering** - Only the visible part of a long document is painted each frame
- **Typography** - 1.5x line height for optimal readability (WCAG 2.1 compliant)

*200+ languages via syntect, and mermaid diagrams rendered natively*

![Syntax highlighting and a mermaid sequence diagram](screenshots/syntax-highlighting.png)

*Tables use the full width of the pane: wide tables wrap their cell text, small ones stretch, and both re-fit as you resize the window or the sidebars*

![Tables filling the document pane](screenshots/tables.png)

*A leading `---` block renders as a key/value table rather than raw text; nested items and folded scalars keep their source spelling*

![YAML frontmatter rendered as a key/value table](screenshots/frontmatter.png)

*Arabic and Hebrew blocks are right-aligned, list markers move to the right, and Latin words inside them keep their place*

![Right-to-left paragraphs, lists and a table](screenshots/rtl.png)

### Navigation
- **Tab System** - Open multiple documents with tab bar (Ctrl+Click links to open in new tab)
- **In-Document Search (Ctrl+F)** - Find bar with inline highlights, Enter/Shift+Enter to cycle matches
- **File Explorer** - Hierarchical sidebar with lazy-loading directories, sorted by name or date; freely resizable, width remembered
- **Open Folder** - Use File → Open Folder… to choose and persist the file explorer root
- **Outline Sidebar** - Click-to-navigate table of contents from document headers; freely resizable, width remembered
- **Navigation Buttons** - Back/forward buttons in title bar for quick history navigation
- **Per-Tab History** - Independent back/forward navigation within each tab (Alt+Left/Right)
- **Internal Links** - Navigate between markdown files with relative links

*Find bar with inline highlights and a match counter; Enter and Shift+Enter cycle*

![In-document search](screenshots/search.png)

### View
- **Dark & Light Themes** - Toggle with Ctrl+D
- **Zoom** - 50% to 300% zoom (Ctrl++/-/0 or Ctrl+Scroll)
- **Full Width** - On by default; toggle between the full content pane and a reading-width column (Ctrl+Shift+W); prose and tables follow it together
- **Formula Size** - Scale rendered math from 100% to 150% (View → Formula Size), independent of UI zoom
- **Tab Bar** - Hide it with View → Show Tab Bar; it comes back while a tab is in Edit mode
- **Keyboard Scrolling** - Scroll documents with ↑/↓ by line or Page Up/Page Down by page when the find bar is closed
- **Live Reload** - Auto-refresh on file changes (enabled by default)
- **Custom Colors** - Customize highlight and link text colors (View → Colors…)
- **Font Selection** - Browse and apply installed system fonts via View menu (searchable, persistent)
- **Smooth Text Rendering** - Optional toggle (View → Smooth Text Rendering) that disables glyph pixel-snapping for less jagged-looking text at fractional zoom levels

*Full Width (Ctrl+Shift+W), fonts, Formula Size and the colour picker all live in the View menu*

![The View menu](screenshots/view-menu.png)

*The same document on both themes — a mermaid flowchart, highlighted code and both sidebars*

| Dark | Light |
|---|---|
| ![Dark mode](screenshots/dark-mode.png) | ![Light mode](screenshots/light-mode.png) |

### Editing
- **Edit Mode** - Switch the active tab between Preview and Edit (Ctrl+E or the Preview/Edit toggle) and save with Ctrl+S

### Usability
- **Drag and Drop** - Drop markdown files onto the window to open
- **Native Dialogs** - System file and folder picker integration
- **Welcome Page & Recent Files** - Open files or folders from the idle screen and reopen recent documents
- **Session Persistence** - Remembers open tabs, theme, zoom, and sidebar state
- **Cross-Platform** - Linux (X11 and Wayland), Windows, and macOS on Apple Silicon

## Keyboard Shortcuts

### Tab Management

| Shortcut | Action |
|----------|--------|
| Ctrl+T | Open file in new tab |
| Ctrl+W | Close current tab |
| Ctrl+Tab | Next tab |
| Ctrl+Shift+Tab | Previous tab |
| Ctrl+1-9 | Switch to tab 1-9 |

### Navigation

| Shortcut | Action |
|----------|--------|
| Alt+Left | Navigate back in history |
| Alt+Right | Navigate forward in history |
| Click link | Navigate in current tab |
| Ctrl+Click link | Open link in new tab |

### Search

| Shortcut | Action |
|----------|--------|
| Ctrl+F | Open find bar (or refocus if already open) |
| Enter / ↓ | Jump to next match |
| Shift+Enter / ↑ | Jump to previous match |
| Esc | Close find bar and clear highlights |

### View

| Shortcut | Action |
|----------|--------|
| Ctrl+D | Toggle dark/light mode |
| Ctrl+Shift+W | Toggle full width / reading column |
| Ctrl+Shift+E | Toggle file explorer |
| Ctrl+Shift+O | Toggle outline sidebar |
| Ctrl++ / Ctrl+= | Zoom in |
| Ctrl+- | Zoom out |
| Ctrl+0 | Reset zoom to 100% |
| ↑ / ↓ (when find bar is closed) | Scroll document up/down by line |
| Page Up / Page Down | Scroll document up/down by page |
| Ctrl+Scroll | Zoom with mouse wheel |
| Shift+Scroll over a wide table | Scroll the table horizontally |

### File Operations

| Shortcut | Action |
|----------|--------|
| Ctrl+O | Open file in new tab |
| Ctrl+E | Toggle Preview / Edit for the active tab |
| Ctrl+S | Save the active tab (Edit mode) |
| F5 | Toggle file watching |
| Ctrl+Q | Quit application |

## Installation

### Quick Install (Linux / macOS) — recommended

Downloads the prebuilt binary for your platform, verifies its SHA256, and installs the binary to `~/.local/bin` plus bundled third-party notices under `~/.local/share/licenses/md-viewer`. No compilation, takes seconds.

```bash
curl -fsSL https://raw.githubusercontent.com/aydiler/md-viewer/main/scripts/install.sh | sh
```

Supports Linux x86_64 and macOS arm64 (Apple Silicon). Set `INSTALL_DIR=/usr/local/bin` to install elsewhere. Intel Macs need to build from source via `cargo install md-viewer`.

On Linux, **Open File** and **Open Folder** need either a working XDG Desktop
Portal FileChooser (plus the `gdbus` command used to detect it), or Python 3
with Tkinter for the fallback dialog. You can check the fallback with
`python3 -c 'import tkinter'`. Common Tkinter package names are `python3-tk` on
Debian/Ubuntu, `python3-tkinter` on Fedora/RHEL, and `python` plus `tk` on Arch.

> **macOS Gatekeeper note:** binaries are not yet signed/notarized. If macOS refuses to run the app, run:
> `xattr -d com.apple.quarantine ~/.local/bin/md-viewer`

### Snap Store

```bash
sudo snap install md-viewer
```

Auto-updates via snapd.

### Arch Linux (AUR)

```bash
yay -S md-viewer-bin    # prebuilt release binary
yay -S md-viewer-git    # builds from the latest main commit
```

`md-viewer-bin` installs the binary from the latest GitHub release. `md-viewer-git` builds the latest `main` commit (rolling). Both update with your normal system update.

### Flatpak / Flathub

Once published to Flathub:

```bash
flatpak install flathub io.github.aydiler.md-viewer
```

(Flathub submission in progress — see `flatpak/` and `PUBLISHING.md`.)

### Windows

Download `md-viewer-<version>-windows-x86_64.zip` from the [latest release](https://github.com/aydiler/md-viewer/releases/latest), extract `md-viewer.exe`, and run it. Verify the included `.sha256` if you'd like.

### Cargo (crates.io) — slower, builds from source

```bash
cargo install md-viewer
```

Compiles locally (~2–3 minutes). Update with `cargo install --force md-viewer`. Requires the system dependencies listed below.

This build has no right-to-left layout: right-to-left blocks are still right-aligned, but their text appears in typed order, and clicks on links inside them can miss. The layout fix lives in a patched copy of epaint (from the not-yet-merged emilk/egui#8822), and `cargo install` builds against the stock epaint from crates.io. Every other channel includes it.

### From Source

```bash
git clone https://github.com/aydiler/md-viewer
cd md-viewer
cargo build --release
make install     # installs to ~/.local/bin (optional)
make uninstall   # removes the local installation
```

### System Dependencies (Arch Linux)

Only needed for `cargo install` / building from source:

```bash
sudo pacman -S --needed \
    base-devel clang pkg-config \
    libxcb libxkbcommon fontconfig zenity \
    xdg-desktop-portal xdg-desktop-portal-gtk \
    python tk
```

## Usage

```bash
# Open a file and return the terminal prompt (live reload is enabled by default)
md-viewer README.md

# Keep the viewer attached to the terminal for debugging/logs
md-viewer --foreground README.md

# Disable live reload
md-viewer README.md --no-watch
```

Run `md-viewer` with no file to start on the welcome page, then choose Open File, Open Folder, or a recent document. Opening a file always creates or focuses its tab; use the `+` button, File → Open File in New Tab…, Ctrl+T, or Ctrl+O. Use File → Open Folder… to choose the file explorer root.

When launched from a terminal, `md-viewer` detaches by default so the shell prompt is available while the window stays open. Use `--foreground` when you want terminal logs or blocking process behavior.

## Technical Details

- **Binary size**: ~50 MB on Linux (~23 MB as the release `.tar.gz`); includes syntax highlighting, the mermaid renderer, math rendering, image support, X11 and Wayland. The snap is a ~90 MB download.
- **Rendering**: only the visible part of the document is painted each frame
- **Memory**: Uses mimalloc for improved allocation performance
- **Platform**: Linux (X11 and Wayland, glow/OpenGL backend), Windows x86_64, macOS arm64

### Built With

- [eframe/egui](https://github.com/emilk/egui) - Immediate mode GUI framework
- [egui_commonmark](https://github.com/lampsitter/egui_commonmark) - Markdown rendering (vendored fork with typography, math, and alignment improvements)
- [emojis](https://crates.io/crates/emojis) - GitHub/gemoji shortcode lookup data (`(MIT OR Apache-2.0) AND Unicode-3.0`; see `THIRD_PARTY_NOTICES`)
- [typst](https://github.com/typst/typst) + [mitex](https://github.com/mitex-rs/mitex) - LaTeX math rendering (LaTeX → typst → rasterized inline)
- [merman](https://github.com/Latias94/merman) - Mermaid diagram rendering
- [syntect](https://github.com/trishume/syntect) - Syntax highlighting
- [notify](https://github.com/notify-rs/notify) - File watching
- [rfd](https://github.com/PolyMeilex/rfd) - Native file dialogs

## License

MIT
