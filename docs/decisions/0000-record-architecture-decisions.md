# ADR-0000: Record Architecture Decisions

**Status:** Accepted
**Date:** 2026-01-22
**Deciders:** Ahmet

## Context

We need to record the architectural decisions made on this project so that future contributors (and future-us) understand why things are the way they are. Without this record, decisions get lost in commit history, devlogs, or tribal knowledge.

## Decision Drivers

- Solo developer project with detailed devlogs already capturing implementation
- Need to distinguish "what was built" (devlogs) from "why this approach was chosen" (ADRs)
- Decisions should be discoverable without reading all devlogs

## Considered Options

### Option 1: Continue using devlogs only

Keep architectural decisions embedded in feature devlogs.

**Pros:**
- No new process
- All context in one place per feature

**Cons:**
- Hard to find specific decisions later
- Devlogs focus on implementation, not alternatives considered
- Cross-cutting decisions don't belong to any single feature

### Option 2: Adopt MADR (Markdown Any Decision Records)

Create a `docs/decisions/` folder with numbered markdown files following MADR format.

**Pros:**
- Standardized format with options/pros/cons
- Easy to reference (ADR-0001)
- Discoverable via filesystem
- Compatible with existing devlog workflow

**Cons:**
- Another thing to maintain
- Some overlap with devlogs

## Decision

Adopt MADR format in `docs/decisions/`. ADRs capture decisions with multiple considered options. Devlogs continue to capture implementation details and discoveries.

**When to create an ADR:**
- Choosing between multiple valid approaches
- Decisions that affect project structure or tooling
- Deviations from common practices that need explanation

**When to use devlogs instead:**
- Implementation details and API discoveries
- Single-feature scope without major alternatives

## Consequences

### Positive

- Key decisions are documented with rationale
- Future refactoring can reference original reasoning
- Clear distinction between "what" (devlogs) and "why" (ADRs)

### Negative

- Slight overhead for major decisions
- Need to decide which document type to use

## Related

- `docs/devlog/` - Feature implementation details
- `docs/LESSONS.md` - Tactical fixes and gotchas
