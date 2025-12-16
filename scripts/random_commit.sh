#!/bin/bash

# Random commit and push script
# Usage: ./random_commit.sh [directory]

TARGET_DIR="${1:-.}"

cd "$TARGET_DIR" || exit 1

# Check if we're in a FUSE-mounted src directory, if so go to parent
if [[ "$(pwd)" == *"/src" ]] && [ -f "../.git/config" ]; then
    echo "Detected FUSE workspace, switching to git root..."
    GIT_ROOT="$(cd .. && pwd)"
    cd "$GIT_ROOT" || exit 1
    SEARCH_DIR="src"
else
    SEARCH_DIR="."
fi

# Find all regular files (excluding .git directory and hidden files)
FILES=($(find "$SEARCH_DIR" -type f -not -path '*/\.git/*' -not -path '*/\.*' 2>/dev/null))

if [ ${#FILES[@]} -eq 0 ]; then
    echo "No files found to modify"
    exit 1
fi

# Pick a random file
RANDOM_FILE="${FILES[$RANDOM % ${#FILES[@]}]}"
# Remove leading ./ or src/ for cleaner display
DISPLAY_FILE="${RANDOM_FILE#./}"
DISPLAY_FILE="${DISPLAY_FILE#src/}"
echo "Selected file: $DISPLAY_FILE"

# Generate random content
RANDOM_CONTENT="Random modification at $(date): $RANDOM"

# Modify the file by appending a line
echo "" >> "$RANDOM_FILE"
echo "// $RANDOM_CONTENT" >> "$RANDOM_FILE"

echo "Modified: $RANDOM_FILE"

# Stage the file - use relative path from git root
RELATIVE_FILE="${RANDOM_FILE#src/}"
RELATIVE_FILE="${RELATIVE_FILE#./}"

# Force git to notice the change by updating the index
git update-index --no-assume-unchanged "$RELATIVE_FILE" 2>/dev/null || true

# Add with -f to force
git add -f "$RELATIVE_FILE"

# Check if anything was actually staged
if ! git diff --cached --quiet 2>/dev/null; then
    # Commit with a message
    COMMIT_MSG="Random update to $(basename $RANDOM_FILE) - $(date +%Y%m%d-%H%M%S)"
    git commit -m "$COMMIT_MSG"
else
    echo "Warning: No changes were staged, trying alternative approach..."
    # Try committing with -a to include all changes
    COMMIT_MSG="Random update to $(basename $RANDOM_FILE) - $(date +%Y%m%d-%H%M%S)"
    git commit -a -m "$COMMIT_MSG"
fi

# Push to origin
echo "Pushing to origin..."
git push origin master

echo "Done!"
