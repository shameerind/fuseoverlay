#!/bin/bash

# Create 100 test files with 3KB each, commit and push
# Usage: ./create_100_files.sh [directory]

TARGET_DIR="${1:-.}"

cd "$TARGET_DIR" || exit 1

# Generate random directory name
RANDOM_DIR="$(pwd)/test_files_$(date +%s)_$$"

# Check if we're in a FUSE-mounted workspace
# If so, we need to work in the actual git repository, not the FUSE mount
if [ -f ".git/config" ]; then
    # We're in the git root, check if src is a FUSE mount
    if mountpoint -q src 2>/dev/null; then
        echo "Warning: src is FUSE-mounted. Files will be created in git working tree."
        echo "You may need to unmount and remount to see them in FUSE."
        WORK_DIR="$RANDOM_DIR"
    else
        WORK_DIR="$RANDOM_DIR"
    fi
elif [[ "$(pwd)" == *"/src" ]] && [ -f "../.git/config" ]; then
    # We're in a FUSE-mounted src directory
    echo "Detected FUSE workspace. Creating files directly in git repository..."
    GIT_ROOT="$(cd .. && pwd)"
    cd "$GIT_ROOT" || exit 1
    WORK_DIR="$RANDOM_DIR"
else
    WORK_DIR="$RANDOM_DIR"
fi

# Create directory for test files
mkdir -p "$WORK_DIR"
echo "Creating 10 files in $WORK_DIR..."

# Generate 10 files with 3KB each
for i in {1..10}; do
    FILE="$WORK_DIR/testfile_$(printf "%03d" $i).txt"
    
    # Generate approximately 3KB of text (about 3000 characters)
    echo "Test file $i" > "$FILE"
    echo "Generated at: $(date)" >> "$FILE"
    echo "Random content below:" >> "$FILE"
    echo "Test file $i" > "$FILE"
    echo "Generated at: $(date)" >> "$FILE"
    echo "Random content below:" >> "$FILE"
    echo "Test file $i" > "$FILE"
    echo "Generated at: $(date)" >> "$FILE"
    echo "Random content below:" >> "$FILE"
    echo "" >> "$FILE"
    
    # Generate random text to fill 3KB
    # Each line is about 80 chars, need about 37 lines for 3KB
    for j in {1..10}; do
        echo "Line $j: $(head -c 70 /dev/urandom | base64 | tr -d '\n')" >> "$FILE"
    done
    
    printf "\rCreated %d/10 files..." $i
done

echo ""
echo "All files created!"

# Stage all files including any modified and untracked files
echo "Staging files..."
git add "$WORK_DIR"

# Also stage any modified and untracked files in the repository
echo "Staging all modified and untracked files..."
git add -A

# Check what will be committed
FILE_COUNT=$(git diff --cached --name-only | wc -l)
echo "Files to commit: $FILE_COUNT"

# Commit
echo "Committing..."
COMMIT_MSG="Add 100 test files (3KB each) - $(date +%Y%m%d-%H%M%S)"
git commit -m "$COMMIT_MSG" || git commit -a -m "$COMMIT_MSG"

# Push
echo "Pushing to origin..."
git push origin master

echo ""
echo "Done! Created 100 files in $WORK_DIR"
