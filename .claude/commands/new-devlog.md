---
description: Create a new numbered devlog from template
allowed-tools: Bash, Read, Write
---

Create a new devlog for a feature or task.

**Steps:**

1. Find the next devlog number:
```bash
ls docs/devlog/[0-9]*.md 2>/dev/null | sort | tail -1
```

2. Read the template:
```bash
cat docs/devlog/TEMPLATE.md
```

3. Create the new devlog file at `docs/devlog/NNN-$ARGUMENTS.md` where:
   - NNN is the next sequential number (zero-padded to 3 digits)
   - $ARGUMENTS is the kebab-case name provided by the user

4. Update the new devlog:
   - Set the branch name to current branch
   - Set today's date
   - Keep status as "In Progress"

5. Report the created file path.

**Example:** `/new-devlog outline-improvements` creates `docs/devlog/010-outline-improvements.md`
