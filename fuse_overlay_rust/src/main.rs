mod types;
mod metrics;
mod cache;
mod node_cache;
mod prefetch;
mod file_ops;
mod dir_ops;
mod gitfs;

use anyhow::{Context, Result};
use fuser::MountOption;
use gitfs::GitFsOverlay;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn main() -> Result<()> {
    let repo = std::env::args()
        .nth(1)
        .context("usage: git_fuse_overlay <repo> <mountpoint>")?;
    let mountpoint = std::env::args()
        .nth(2)
        .context("usage: git_fuse_overlay <repo> <mountpoint>")?;
    std::fs::create_dir_all(&mountpoint)?;

    let mountpoint_path = PathBuf::from(&mountpoint);
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let mp = mountpoint_path.clone();

    // Write PID to file for later unmounting
    let pid = std::process::id();
    let pid_file = PathBuf::from(&mountpoint).join("../.git/fuse_pid");
    std::fs::write(&pid_file, pid.to_string())
        .context("Failed to write PID file")?;
    eprintln!("FUSE filesystem PID: {} (saved to {:?})", pid, pid_file);

    // Set up signal handler
    ctrlc::set_handler(move || {
        eprintln!("Received signal, unmounting...");
        r.store(false, Ordering::SeqCst);
        // Unmount the filesystem with lazy unmount to avoid "device busy" errors
        let _ = std::process::Command::new("fusermount")
            .arg("-uz")
            .arg(&mp)
            .status();
        std::process::exit(0);
    }).context("Error setting Ctrl-C handler")?;

    eprintln!("Mounting {} at {}", repo, mountpoint);
    let fs = GitFsOverlay::new(Path::new(&repo))?;
    
    // This blocks until the filesystem is unmounted
    fuser::mount2(
        fs,
        mountpoint,
        &[
            MountOption::RO,
            MountOption::FSName("sb_overlay".into()),
            MountOption::AllowOther,
            MountOption::CUSTOM("nonempty".into()),
        ],
    )?;

    eprintln!("Filesystem unmounted");
    Ok(())
}
