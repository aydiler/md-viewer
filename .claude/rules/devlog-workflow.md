# Devlog Workflow

Every feature implementation should create a devlog file in `docs/devlog/` to capture:
- Implementation status and scope
- Key discoveries and API learnings
- Architecture decisions
- Future improvement ideas

## Automatic Devlog Creation

When creating a feature worktree or starting non-trivial work, **automatically create a devlog**:

```bash
# Find next number
LAST=$(ls docs/devlog/[0-9]*.md 2>/dev/null | sort | tail -1 | grep -oP '\d{3}' | head -1)
NEXT=$(printf "%03d" $((10#$LAST + 1)))

# Create from template
cp docs/devlog/TEMPLATE.md docs/devlog/${NEXT}-<feature-name>.md
```

Then update the new file with:
- Branch name
- Today's date
- Initial feature checklist

Use `/new-devlog <name>` as an explicit shortcut if needed.

## Devlog Structure

```
docs/devlog/
├── TEMPLATE.md           # Copy this for new features
├── 001-link-navigation.md
├── 002-feature-name.md
└── ...
```

## Naming Convention

`NNN-feature-name.md` where NNN is a zero-padded sequential number.

## When to Create

1. **At worktree creation**: Create immediately (automated - see above)
2. **During implementation**: Document discoveries as you learn them
3. **At completion**: Update status, add architecture details, note future improvements

## What Requires a Devlog

Create a devlog for ANY work that:
- Adds new structs, fields, or functions
- Changes user-visible behavior
- Introduces new API usage patterns

**"Phase A/B/C" designations don't exempt documentation. Infrastructure is a feature.**

## Key Sections to Always Include

- **Key Discoveries**: Non-obvious solutions, API quirks, gotchas
- **Architecture**: New fields, functions, data flow changes
- **Future Improvements**: Ideas that emerged but weren't implemented
