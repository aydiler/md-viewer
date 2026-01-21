# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Branch Context

**Branch:** `main`
**Purpose:** Stable, production-ready code

## Project Documentation

These docs are versioned with code and imported via `@path`:

- @docs/ARCHITECTURE.md - Core components, libraries, rendering flow
- @docs/KEYBOARD_SHORTCUTS.md - All keyboard shortcuts
- @docs/TARGET_METRICS.md - Performance targets and planned features

## Claude Instructions

Stable instructions are auto-loaded from `.claude/rules/`:
- `build-commands.md` - cargo build, run, clippy, make install
- `devlog-workflow.md` - How to document feature implementations
- `worktree-workflow.md` - How to create and manage worktrees
- `system-dependencies.md` - Arch Linux packages

## Branch-Specific Notes

This is the main branch. All new work should happen in feature worktrees.
Use `/feature <description>` or create a worktree manually.
