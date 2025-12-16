#!/bin/bash

# Usage: ./create_myws.sh <path_to_master_repo> <workspace_name>

if [ $# -ne 2 ]; then
    echo "Usage: $0 <path_to_master_repo> <wsname>"
    exit 1
fi

MASTER_REPO="$1"
WS_NAME="$2"

# Ensure master repo exists
if [ ! -d "$MASTER_REPO/.git" ]; then
    echo "Error: $MASTER_REPO is not a valid git repository."
    exit 1
fi

# Ensure workspace directory does not already exist
if [ -d "$WS_NAME" ]; then
    echo "Error: Directory $WS_NAME already exists."
    exit 1
fi

MASTER_REPO=$(realpath "$MASTER_REPO")

# Step 1: Clone master repo into workspace (using shared objects for speed if local)
echo "Cloning master repo into workspace '$WS_NAME'..."
if [[ "$MASTER_REPO" == /* ]] || [[ "$MASTER_REPO" == ./* ]]; then
    # Local repo - use shared objects for instant clone
    git clone --no-checkout --shared "$MASTER_REPO" "$WS_NAME"
else
    # Remote repo - normal clone (or use shallow clone for speed)
    git clone --no-checkout --depth 1 "$MASTER_REPO" "$WS_NAME"
fi

cd "$WS_NAME" || exit 1

# Step 2: Create src directory for overlay
mkdir -p src

# Step 3: Start git_fuse_overlay on src directory
echo "Starting git_fuse_overlay for workspace '$WS_NAME'..."
/workspaces/git/git_fuse_overlay/target/release/git_fuse_overlay "$MASTER_REPO" "$(pwd)/src" &
FUSE_PID=$!

# Wait for mount to be ready
echo "Waiting for filesystem to mount..."
for i in {1..10}; do
    if mountpoint -q "$(pwd)/src" 2>/dev/null; then
        echo "Filesystem mounted successfully"
        break
    fi
    sleep 0.5
done

if ! mountpoint -q "$(pwd)/src"; then
    echo "Error: Failed to mount filesystem"
    kill $FUSE_PID 2>/dev/null
    exit 1
fi

# Step 4: Configure worktree to src
git config core.worktree "$(pwd)/src"

# Step 5: Initialize the index to match HEAD (without checking out files)
# The files are already visible via FUSE, we just need git to know about them
HEAD_COMMIT=$(git --git-dir="$MASTER_REPO/.git" rev-parse HEAD)
git read-tree "$HEAD_COMMIT"

# Enable git filesystem monitor to improve performance
git config core.fsmonitor true
git config core.untrackedCache true

# Speed up commits by not auto-updating index before commit
git config commit.verbose false

# Ignore file mode changes (FUSE provides permissions, not stored per-file)
git config core.fileMode false

echo ""
echo "=========================================="
echo "Workspace '$WS_NAME' is ready!"
echo "=========================================="
echo "FUSE PID: $FUSE_PID"
echo ""
echo "IMPORTANT: Always work from the src/ directory:"
echo "  cd $WS_NAME/src"
echo ""
echo "Git Tips for FUSE Performance:"
echo "  - Commit specific files: git commit <file1> <file2> -m 'msg'"
echo "  - Or use: ./git-fast-commit.sh -m 'msg'"
echo "  - Add specific files: git add <specific_files>"
echo "  - Check status of subset: git status <directory>"
echo "=========================================="
