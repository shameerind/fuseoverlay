use dashmap::DashMap;
use fuser::{FileAttr, FileType};
use git2::{ObjectType, Repository, FileMode};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use crate::types::{Node, ROOT_INO, i32_to_filemode, git_mode_to_perm};
use crate::cache::LruCache;

pub struct NodeCache {
    nodes: DashMap<u64, Node>,
    ino_cache: DashMap<PathBuf, u64>,
    path_to_ino: DashMap<PathBuf, u64>,
    next_ino: AtomicU64,
}

impl NodeCache {
    pub fn new() -> Self {
        let cache = Self {
            nodes: DashMap::new(),
            ino_cache: DashMap::new(),
            path_to_ino: DashMap::new(),
            next_ino: AtomicU64::new(ROOT_INO + 1),
        };
        
        // Insert root node
        cache.nodes.insert(
            ROOT_INO,
            Node {
                ino: ROOT_INO,
                kind: FileType::Directory,
                size: 0,
                path: PathBuf::new(),
                git_mode: Some(FileMode::Tree),
            },
        );
        cache.path_to_ino.insert(PathBuf::new(), ROOT_INO);
        
        cache
    }

    pub fn alloc_ino(&self, path: &Path) -> u64 {
        if let Some(ino) = self.ino_cache.get(path) {
            *ino
        } else {
            let ino = self.next_ino.fetch_add(1, Ordering::Relaxed);
            self.ino_cache.insert(path.to_path_buf(), ino);
            ino
        }
    }

    pub fn get_node(&self, ino: &u64) -> Option<Node> {
        self.nodes.get(ino).map(|n| n.clone())
    }

    pub fn insert_node(&self, ino: u64, node: Node) {
        self.nodes.insert(ino, node.clone());
        self.path_to_ino.insert(node.path.clone(), ino);
    }

    pub fn remove_node(&self, path: &Path) -> Option<u64> {
        if let Some((_, ino)) = self.path_to_ino.remove(path) {
            self.nodes.remove(&ino);
            Some(ino)
        } else {
            None
        }
    }

    pub fn get_ino_by_path(&self, path: &Path) -> Option<u64> {
        self.path_to_ino.get(path).map(|i| *i)
    }

    pub fn node_to_attr(&self, node: &Node) -> FileAttr {
        let perm = match &node.git_mode {
            Some(mode) => git_mode_to_perm(*mode),
            None => match node.kind {
                FileType::Directory => 0o755,
                _ => 0o644,
            },
        };

        FileAttr {
            ino: node.ino,
            size: node.size,
            blocks: (node.size + 511) / 512,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: node.kind,
            perm,
            nlink: 1,
            uid: unsafe { libc::geteuid() },
            gid: unsafe { libc::getegid() },
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }

    pub fn lookup_path(
        &self,
        path: &Path,
        overlay: &Arc<LruCache>,
        repo: &Repository,
        head: git2::Oid,
    ) -> Option<Node> {
        // Check cached inode first (preserves directory type)
        if let Some(ino) = self.path_to_ino.get(path) {
            return self.nodes.get(&*ino).map(|n| n.clone());
        }

        // Check overlay (but only for files, directories are in nodes)
        let path_buf = path.to_path_buf();
        if let Some(data) = overlay.get(&path_buf) {
            let ino = self.alloc_ino(path);
            let node = Node {
                ino,
                kind: FileType::RegularFile,
                size: data.len() as u64,
                path: path_buf.clone(),
                git_mode: None,
            };
            self.nodes.insert(ino, node.clone());
            self.path_to_ino.insert(path_buf, ino);
            return Some(node);
        }

        // Git traversal
        let commit = repo.find_commit(head).ok()?;
        let mut curr_tree = commit.tree().ok()?;
        let mut curr_path = PathBuf::new();
        let mut last_node: Option<Node> = None;

        for comp in path.iter() {
            let comp_str = comp.to_str()?;
            curr_path.push(comp);

            let tree_next = {
                let entry = curr_tree.get_name(comp_str)?;
                let kind = match entry.kind() {
                    Some(ObjectType::Tree) => FileType::Directory,
                    Some(ObjectType::Blob) => FileType::RegularFile,
                    _ => return None,
                };

                let size = if kind == FileType::RegularFile {
                    entry.to_object(repo).ok()?.peel_to_blob().ok()?.size() as u64
                } else {
                    0
                };

                let node = Node {
                    ino: self.alloc_ino(&curr_path),
                    kind,
                    size,
                    path: curr_path.clone(),
                    git_mode: Some(i32_to_filemode(entry.filemode())),
                };
                self.nodes.insert(node.ino, node.clone());
                self.path_to_ino.insert(curr_path.clone(), node.ino);
                last_node = Some(node.clone());

                if kind == FileType::Directory {
                    entry.to_object(repo).ok()?.peel_to_tree().ok()?
                } else {
                    return last_node;
                }
            };
            curr_tree = tree_next;
        }

        last_node
    }
}
