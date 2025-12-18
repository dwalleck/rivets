#!/bin/bash
# Install git hooks for the rivets project

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Verify we're in a git repository
if ! git -C "$REPO_ROOT" rev-parse --git-dir > /dev/null 2>&1; then
    echo "ERROR: Not a git repository: $REPO_ROOT"
    exit 1
fi

HOOKS_DIR="$(git -C "$REPO_ROOT" rev-parse --git-dir)/hooks"

echo "Installing git hooks..."

# Install commit-msg hook (backup existing if present)
if [[ -f "$HOOKS_DIR/commit-msg" ]]; then
    echo "  - Backing up existing commit-msg hook to commit-msg.bak"
    cp "$HOOKS_DIR/commit-msg" "$HOOKS_DIR/commit-msg.bak"
fi
cp "$SCRIPT_DIR/commit-msg" "$HOOKS_DIR/commit-msg"
chmod +x "$HOOKS_DIR/commit-msg"
echo "  - commit-msg hook installed"

echo ""
echo "Git hooks installed successfully!"
echo ""
echo "The following hooks are now active:"
echo "  - commit-msg: Validates conventional commit format"
echo ""
echo "To bypass hooks (not recommended): git commit --no-verify"
