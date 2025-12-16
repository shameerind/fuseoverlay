use anyhow::{Context, Result};
use fuser::*;
use git2::{Repository, FileMode};
use libc::ENOENT;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use crate::types::Node;
use crate::metrics::{debug, Metrics};
use crate::node_cache::NodeCache;
use crate::cache::LruCache;
use crate::{prefetch, file_ops, dir_ops};

const TTL: Duration = Duration::from_secs(1);

// Default cache limits: 2048MB and 50000 files
const DEFAULT_MAX_CACHE_BYTES: usize = 2048 * 1024 * 1024;
const DEFAULT_MAX_CACHE_ENTRIES: usize = 50_000;

pub struct GitFsOverlay {
    repo: Repository,
    repo_path: PathBuf,
    head: git2::Oid,
    node_cache: NodeCache,
    overlay: Arc<LruCache>,
    metrics: Arc<Metrics>,
}

impl GitFsOverlay {
    pub fn new(repo_path: &Path) -> Result<Self> {
        let repo = Repository::open(repo_path)?;
        let head = repo.head()?.target().context("invalid HEAD")?;

        Ok(GitFsOverlay {
            repo,
            repo_path: repo_path.to_path_buf(),
            head,
            node_cache: NodeCache::new(),
            overlay: Arc::new(LruCache::new(DEFAULT_MAX_CACHE_BYTES, DEFAULT_MAX_CACHE_ENTRIES)),
            metrics: Arc::new(Metrics::default()),
        })
    }

    #[allow(dead_code)]
    pub fn with_cache_limits(repo_path: &Path, max_bytes: usize, max_entries: usize) -> Result<Self> {
        let repo = Repository::open(repo_path)?;
        let head = repo.head()?.target().context("invalid HEAD")?;

        Ok(GitFsOverlay {
            repo,
            repo_path: repo_path.to_path_buf(),
            head,
            node_cache: NodeCache::new(),
            overlay: Arc::new(LruCache::new(max_bytes, max_entries)),
            metrics: Arc::new(Metrics::default()),
        })
    }

    fn prefetch_directory(&self, dir_path: &Path) {
        prefetch::prefetch_directory(
            self.repo_path.clone(),
            dir_path.to_path_buf(),
            self.head,
            self.overlay.clone(),
            self.metrics.clone(),
        );
    }
}

impl Filesystem for GitFsOverlay {
    fn flush(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        reply.ok();
    }

    fn init(&mut self, _: &Request<'_>, _: &mut KernelConfig) -> Result<(), libc::c_int> {
        debug!("GitFS Overlay mounted");
        Ok(())
    }

    fn lookup(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        debug!("[LOOKUP] parent={}, name={:?}", parent, name);
        let parent_node = match self.node_cache.get_node(&parent) {
            Some(n) => n,
            None => {
                debug!("[LOOKUP] parent not found");
                return reply.error(ENOENT);
            }
        };

        let path = parent_node.path.join(name);
        debug!("[LOOKUP] looking up path: {:?}", path);
        match self.node_cache.lookup_path(&path, &self.overlay, &self.repo, self.head) {
            Some(n) => {
                debug!("[LOOKUP] found: {:?}, kind={:?}", path, n.kind);
                
                // If it's a directory, prefetch its contents
                if n.kind == FileType::Directory {
                    self.prefetch_directory(&n.path);
                }
                
                reply.entry(&TTL, &self.node_cache.node_to_attr(&n), 0)
            },
            None => {
                debug!("[LOOKUP] not found: {:?}", path);
                reply.error(ENOENT)
            }
        }
    }

    fn getattr(&mut self, _: &Request<'_>, ino: u64, _: Option<u64>, reply: ReplyAttr) {
        match self.node_cache.get_node(&ino) {
            Some(n) => reply.attr(&TTL, &self.node_cache.node_to_attr(&n)),
            None => reply.error(ENOENT),
        }
    }

    fn readdir(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        _: u64,
        offset: i64,
        reply: ReplyDirectory,
    ) {
        let node = match self.node_cache.get_node(&ino) {
            Some(n) => n,
            None => {
                debug!("[READDIR] inode not found");
                return reply.error(ENOENT);
            }
        };
        
        dir_ops::read_directory(
            &node,
            offset,
            &self.node_cache,
            &self.overlay,
            &self.repo,
            self.head,
            reply,
        );
        
        // Trigger prefetch for this directory
        self.prefetch_directory(&node.path);
    }

    fn read(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        _: u64,
        offset: i64,
        size: u32,
        _: i32,
        _: Option<u64>,
        reply: ReplyData,
    ) {
        let node = match self.node_cache.get_node(&ino) {
            Some(n) => n,
            None => {
                debug!("[READ] inode not found");
                return reply.error(ENOENT);
            }
        };
        
        file_ops::read_file(
            &node,
            offset,
            size,
            &self.overlay,
            &self.repo,
            self.head,
            &self.metrics,
            reply,
        );
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _flags: u32,
        _write_flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite
    ) {
        file_ops::write_file(
            ino,
            offset,
            data,
            &self.node_cache,
            &self.overlay,
            &self.repo,
            self.head,
            reply,
        );
    }

    fn mkdir(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        debug!("[MKDIR] parent={}, name={:?}", parent, name);
        let parent_node = match self.node_cache.get_node(&parent) {
            Some(n) => n,
            None => {
                debug!("[MKDIR] parent not found");
                return reply.error(ENOENT);
            }
        };

        let path = parent_node.path.join(name);
        debug!("[MKDIR] creating directory: {:?}", path);
        let ino = self.node_cache.alloc_ino(&path);
        
        // Mark directory in overlay as empty vec to make it visible
        self.overlay.insert(path.clone(), Vec::new());
        
        let node = Node {
            ino,
            kind: FileType::Directory,
            size: 0,
            path: path.clone(),
            git_mode: Some(FileMode::Tree),
        };
        
        self.node_cache.insert_node(ino, node.clone());
        reply.entry(&TTL, &self.node_cache.node_to_attr(&node), 0);
    }

    fn create(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        debug!("[CREATE] parent={}, name={:?}", parent, name);
        let parent_node = match self.node_cache.get_node(&parent) {
            Some(n) => n,
            None => {
                debug!("[CREATE] parent not found");
                return reply.error(ENOENT);
            }
        };

        let path = parent_node.path.join(name);
        debug!("[CREATE] creating file: {:?}", path);
        let ino = self.node_cache.alloc_ino(&path);
        
        // Create empty file in overlay
        self.overlay.insert(path.clone(), Vec::new());
        
        let node = Node {
            ino,
            kind: FileType::RegularFile,
            size: 0,
            path: path.clone(),
            git_mode: Some(FileMode::Blob),
        };
        
        self.node_cache.insert_node(ino, node.clone());
        reply.created(&TTL, &self.node_cache.node_to_attr(&node), 0, 0, 0);
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let parent_node = match self.node_cache.get_node(&parent) {
            Some(n) => n,
            None => return reply.error(ENOENT),
        };

        let path = parent_node.path.join(name);
        
        // Remove from overlay
        self.overlay.remove(&path);
        
        // Remove from node cache
        self.node_cache.remove_node(&path);
        
        reply.ok();
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let parent_node = match self.node_cache.get_node(&parent) {
            Some(n) => n,
            None => return reply.error(ENOENT),
        };

        let path = parent_node.path.join(name);
        
        // Remove from node cache
        self.node_cache.remove_node(&path);
        
        reply.ok();
    }

    fn rename(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        let parent_node = match self.node_cache.get_node(&parent) {
            Some(n) => n,
            None => return reply.error(ENOENT),
        };
        
        let newparent_node = match self.node_cache.get_node(&newparent) {
            Some(n) => n,
            None => return reply.error(ENOENT),
        };

        let old_path = parent_node.path.join(name);
        let new_path = newparent_node.path.join(newname);
        
        // Move in overlay if exists
        if let Some(data) = self.overlay.remove(&old_path) {
            self.overlay.insert(new_path.clone(), data);
        }
        
        // Update node cache
        if let Some(ino) = self.node_cache.remove_node(&old_path) {
            if let Some(mut node) = self.node_cache.get_node(&ino) {
                node.path = new_path.clone();
                self.node_cache.insert_node(ino, node);
            }
        }
        
        reply.ok();
    }

    fn setattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        _size: Option<u64>,
        _atime: Option<fuser::TimeOrNow>,
        _mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        debug!("[SETATTR] ino={}, size={:?}, mode={:?}", ino, _size, _mode);
        
        // Handle size changes for truncate
        if let Some(size) = _size {
            debug!("[SETATTR] truncating to size {}", size);
            if let Some(node) = self.node_cache.get_node(&ino) {
                let mut content = self.overlay.get(&node.path).unwrap_or_else(Vec::new);
                content.resize(size as usize, 0);
                self.overlay.insert(node.path.clone(), content);
            }
        }
        
        // Return current attributes
        match self.node_cache.get_node(&ino) {
            Some(n) => reply.attr(&TTL, &self.node_cache.node_to_attr(&n)),
            None => reply.error(ENOENT),
        }
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        debug!("[OPEN] ino={}, flags={:#x}", ino, flags);
        match self.node_cache.get_node(&ino) {
            Some(n) => {
                debug!("[OPEN] opened: {:?}", n.path);
                reply.opened(0, flags as u32)
            }
            None => {
                debug!("[OPEN] inode not found");
                reply.error(ENOENT)
            }
        }
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn fsync(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _datasync: bool, reply: ReplyEmpty) {
        reply.ok();
    }
}
