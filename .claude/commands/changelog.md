---
description: Generate CHANGELOG.md (suggested automatically before releases)
allowed-tools: Bash, Write
---

**Note:** Changelog generation is suggested automatically when discussing releases (see release-workflow.md).
Use `/changelog` to explicitly generate/update.

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
