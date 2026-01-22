# Devlog Workflow

Every feature implementation should create a devlog file in `docs/devlog/` to capture:
- Implementation status and scope
- Key discoveries and API learnings
- Architecture decisions
- Future improvement ideas

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

1. **At worktree creation**: Copy TEMPLATE.md immediately (see worktree-workflow.md)
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
