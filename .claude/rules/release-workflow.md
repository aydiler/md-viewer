# Release Workflow

## Automatic Changelog Suggestion

When the user mentions any of these, **suggest generating a changelog**:
- "release", "version", "tag", "publish"
- "what changed", "changelog", "release notes"
- Merging a significant feature to main

Suggest: "Would you like me to generate/update the CHANGELOG.md using git-cliff?"

## Generating Changelog

If git-cliff is installed, generate changelog automatically:

```bash
git-cliff -o CHANGELOG.md
```

If not installed:
```bash
cargo install git-cliff
```

Use `/changelog` as an explicit shortcut if needed.

## Before Tagging a Release

1. Ensure all features are merged to main
2. Generate fresh changelog: `git-cliff -o CHANGELOG.md`
3. Review changelog for accuracy
4. Commit changelog if changed
5. Create version tag: `git tag v0.X.0`
