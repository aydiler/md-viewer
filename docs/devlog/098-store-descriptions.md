# 098 — Store descriptions

**Branch:** docs/descriptions
**Date:** 2026-11-16
**Status:** Done

## Scope
Every channel showed exactly the text in its source file, but that text predated most of 0.3.x.

- **Short line** (crates.io `description`, both AUR `pkgdesc`s, the PUBLISHING.md quote, and the GitHub repo description set by hand with `gh repo edit`). It said "for Linux" although releases ship Windows and macOS builds, and it named neither math nor Mermaid. Now: "Fast, lightweight markdown viewer with LaTeX math, mermaid diagrams, tabs, file explorer and live reload".
- **Snap long description.** Three claims were stale:
  - "resizable table columns": tables fill the pane now, and a drag only overrides that;
  - "60 FPS": never measured for this version;
  - the "full-width toggle": it is on by default and has a shortcut.

  Added right-to-left text, frontmatter, Edit mode, Open Folder, the welcome page, explorer sorting and the View-menu settings. The wording follows the README from #246.
- **Metainfo `<description>`**: the same refresh, as 11 items.

Kept on purpose: the snap and metainfo `<summary>` ("…for Linux"). Both target Linux software centers, and the snap summary is limited to 78 characters.

## Key Discoveries
- The repo has no `.SRCINFO`; `release.yml` regenerates it and rewrites `pkgver` at publish time, so a `pkgdesc` change needs no `pkgrel` bump.
- crates.io, AUR and the Snap Store listing take their text from the repo during the release run, so they show this text from the next tag. The snap listing is synced by `snapcraft upload-metadata --force` in `scripts/snap-ci.sh`.

## Future Improvements
- None.
