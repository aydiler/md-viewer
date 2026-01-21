#!/bin/bash
# Prevent edits on protected branches (main, master)
# Reads file_path from Claude Code JSON input via stdin

# Read JSON from stdin
input=$(cat)

# Extract file_path using jq (or grep/sed fallback)
if command -v jq &>/dev/null; then
    file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty')
else
    file_path=$(echo "$input" | grep -oP '"file_path"\s*:\s*"\K[^"]+')
fi

# Get branch from the file's directory, not CWD
if [ -n "$file_path" ]; then
    dir=$(dirname "$file_path")
    branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null)
else
    branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
fi

if [ "$branch" = "main" ] || [ "$branch" = "master" ]; then
    cat << 'DENY'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Cannot edit files on 'main' branch. Create a feature worktree first:\n\ngit -C ~/markdown-viewer/.bare worktree add ~/markdown-viewer/worktrees/<name> -b feature/<name>"}}
DENY
fi

exit 0
