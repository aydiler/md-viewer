#!/usr/bin/env python3
"""Rewrite the root Cargo.toml into the manifest that crates.io receives.

Used by both `publish-crates` (the real publish) and `crates-package-check`
(the release dry run) in .github/workflows/release.yml, so the package the dry
run verifies is the package the tag publishes.

1. Strip the local-only MCP dependency. Anchored to start-of-line so commented
   `# mcp = ...` / `# egui-mcp-bridge = ...` lines stay untouched and the
   working tree stays clean (regression caught in the v0.1.9 publish).

Until the egui 0.37 upgrade (devlog 089) this also dropped a `color-emoji`
default feature, which needed an API only the vendored epaint 0.33 had (#215).
Stock epaint 0.37 draws color glyphs, so the feature is gone. `cargo publish`
still ignores `[patch.crates-io]`, and `crates-package-check` builds the
published package against stock epaint, so a future patch-only API in the app
fails there rather than on crates.io.
"""
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "Cargo.toml")
text = path.read_text()

text = re.sub(r'(?m)^mcp\s*=\s*\["dep:egui-mcp-bridge"\]', 'mcp = []', text)
text = re.sub(r'(?m)^egui-mcp-bridge\s*=\s*.*\n', '', text)

path.write_text(text)
print(f"{path}: MCP dependency stripped")
