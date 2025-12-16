#!/bin/bash

# Cleanup a git_fuse_overlay workspace

if [ $# -ne 1 ]; then
    echo "Usage: $0 <ws_dir>"
    exit 1
fi

ws_dir="$1"

# Check if workspace is valid
if [ ! -d "$ws_dir/.git" ]; then
    echo "Error: '$ws_dir' is not a valid workspace."
    exit 1
fi

FUSE_PID_FILE="$ws_dir/.git/fuse_pid"

# Read FUSE PID
if [ ! -f "$FUSE_PID_FILE" ]; then
    echo "Error: FUSE PID file not found at $FUSE_PID_FILE."
    exit 1
fi

FUSE_PID=$(cat "$FUSE_PID_FILE")

# Kill the FUSE process safely
if kill -0 "$FUSE_PID" 2>/dev/null; then
    echo "Stopping FUSE process with PID $FUSE_PID..."
    kill "$FUSE_PID"
    sleep 1
    if kill -0 "$FUSE_PID" 2>/dev/null; then
        echo "FUSE process did not terminate, sending SIGKILL..."
        kill -9 "$FUSE_PID"
    fi
    echo "FUSE process stopped."
else
    echo "Warning: No running FUSE process found with PID $FUSE_PID."
fi

# Unmount the FUSE filesystem
MOUNT_POINT="$ws_dir/src"
if mountpoint -q "$MOUNT_POINT"; then
    echo "Unmounting FUSE filesystem at $MOUNT_POINT..."
    fusermount -uz "$MOUNT_POINT"
    if [ $? -eq 0 ]; then
        echo "FUSE filesystem unmounted successfully."
    else
        echo "Warning: Failed to unmount $MOUNT_POINT."
    fi
else
    echo "No FUSE filesystem mounted at $MOUNT_POINT."
fi
sleep 2
fusermount -uz "$MOUNT_POINT"
# Remove PID file
rm -f "$FUSE_PID_FILE"
rm -rf "$ws_dir"
echo "Cleanup complete."
