#!/bin/bash

# Do 10 random commits with 5 second sleep between each

TARGET_DIR="${1:-$(pwd)}"

for i in {1..10}; do
    echo "==================================="
    echo "Commit $i/10"
    echo "==================================="
    /workspaces/git/sbs/random_commit.sh "$TARGET_DIR"
    
    if [ $i -lt 10 ]; then
        echo "Waiting 5 seconds..."
        sleep 5
    fi
done

echo ""
echo "All 10 commits completed!"
