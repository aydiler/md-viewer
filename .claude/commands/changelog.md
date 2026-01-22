---
description: Generate CHANGELOG.md from git history
allowed-tools: Bash, Write
---

Generate or update the CHANGELOG.md using git-cliff.

**Steps:**

1. Check if git-cliff is installed:
```bash
which git-cliff || cargo install git-cliff
```

2. Generate the changelog:
```bash
git-cliff -o CHANGELOG.md
```

3. Show a summary of what was generated:
```bash
head -50 CHANGELOG.md
```

4. Report success and remind user to review before committing.
