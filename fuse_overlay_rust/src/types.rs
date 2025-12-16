use fuser::FileType;
use git2::FileMode;
use std::path::PathBuf;

pub const ROOT_INO: u64 = 1;

#[derive(Clone)]
pub struct Node {
    pub ino: u64,
    pub kind: FileType,
    pub size: u64,
    pub path: PathBuf,
    pub git_mode: Option<FileMode>,
}

pub fn i32_to_filemode(mode: i32) -> FileMode {
    match mode {
        0o100755 => FileMode::BlobExecutable,
        0o100644 => FileMode::Blob,
        0o040000 => FileMode::Tree,
        0o120000 => FileMode::Link,
        0o160000 => FileMode::Commit,
        _ => FileMode::Blob,
    }
}

pub fn git_mode_to_perm(mode: FileMode) -> u16 {
    match mode {
        FileMode::Blob => 0o644,
        FileMode::BlobExecutable => 0o755,
        FileMode::Tree => 0o755,
        _ => 0o644,
    }
}
