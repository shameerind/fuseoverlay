use fuser::{ReplyData, ReplyWrite};
use git2::Repository;
use libc::ENOENT;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use crate::metrics::{debug, Metrics};
use crate::node_cache::NodeCache;
use crate::types::Node;
use crate::cache::LruCache;

pub fn read_file(
    node: &Node,
    offset: i64,
    size: u32,
    overlay: &Arc<LruCache>,
    repo: &Repository,
    head: git2::Oid,
    metrics: &Arc<Metrics>,
    reply: ReplyData,
) {
    debug!("[READ] ino={}, offset={}, size={}", node.ino, offset, size);
    debug!("[READ] path={:?}", node.path);

    // Check overlay first
    if let Some(data) = overlay.get(&node.path) {
        debug!("[READ] reading from overlay, len={}", data.len());
        let off = offset as usize;
        let end = usize::min(off + size as usize, data.len());
        reply.data(&data[off..end]);
        return;
    }

    debug!("[READ] reading from git (on-demand)");
    // Git - on-demand fetch
    let commit = match repo.find_commit(head) {
        Ok(c) => c,
        Err(e) => {
            debug!("[READ] failed to find commit: {}", e);
            return reply.error(libc::EIO);
        }
    };
    let mut curr_tree = match commit.tree() {
        Ok(t) => t,
        Err(e) => {
            debug!("[READ] failed to get tree: {}", e);
            return reply.error(libc::EIO);
        }
    };
    
    if let Some(parent) = node.path.parent() {
        for c in parent.iter() {
            let comp_str = match c.to_str() {
                Some(s) => s,
                None => {
                    debug!("[READ] invalid UTF-8 in path component");
                    return reply.error(libc::EINVAL);
                }
            };
            let tree_next = curr_tree.get_name(comp_str)
                .and_then(|e| e.to_object(repo).ok())
                .and_then(|o| o.peel_to_tree().ok());
            
            curr_tree = match tree_next {
                Some(t) => t,
                None => {
                    debug!("[READ] failed to navigate to parent path");
                    return reply.error(ENOENT);
                }
            };
        }
    }

    let name = match node.path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => {
            debug!("[READ] invalid filename");
            return reply.error(libc::EINVAL);
        }
    };
    
    let blob = match curr_tree.get_name(name)
        .and_then(|e| e.to_object(repo).ok())
        .and_then(|o| o.peel_to_blob().ok()) {
        Some(b) => b,
        None => {
            debug!("[READ] failed to get blob for {}", name);
            return reply.error(ENOENT);
        }
    };

    let data = blob.content();
    let off = offset as usize;
    let end = usize::min(off + size as usize, data.len());
    debug!("[READ] returning {} bytes", end - off);
    
    // Track on-demand fetch
    if offset == 0 && size >= data.len() as u32 {
        metrics.on_demand_count.fetch_add(1, Ordering::Relaxed);
        metrics.on_demand_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
    }
    
    reply.data(&data[off..end]);
}

pub fn write_file(
    ino: u64,
    offset: i64,
    data: &[u8],
    node_cache: &NodeCache,
    overlay: &Arc<LruCache>,
    repo: &Repository,
    head: git2::Oid,
    reply: ReplyWrite,
) {
    debug!("[WRITE] ino={}, offset={}, len={}", ino, offset, data.len());
    
    if let Some(file) = node_cache.get_node(&ino) {
        debug!("[WRITE] path={:?}", file.path);
        let path = &file.path;
        
        // Prefetch original content from git if not in overlay yet
        if !overlay.contains_key(path) && offset == 0 {
            
            if let Ok(commit) = repo.find_commit(head) {
                if let Ok(mut curr_tree) = commit.tree() {
                    if let Some(parent) = path.parent() {
                        for c in parent.iter() {
                            if let Some(comp_str) = c.to_str() {
                                if let Some(next_tree) = curr_tree.get_name(comp_str)
                                    .and_then(|e| e.to_object(repo).ok())
                                    .and_then(|o| o.peel_to_tree().ok()) {
                                    curr_tree = next_tree;
                                }
                            }
                        }
                    }
                    
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if let Some(blob) = curr_tree.get_name(name)
                            .and_then(|e| e.to_object(repo).ok())
                            .and_then(|o| o.peel_to_blob().ok()) {
                            overlay.insert(path.clone(), blob.content().to_vec());
                        }
                    }
                }
            }
        }
        
        let mut content = overlay.get(path).unwrap_or_else(Vec::new);

        if content.len() < offset as usize + data.len() {
            content.resize(offset as usize + data.len(), 0);
        }

        content[offset as usize..offset as usize + data.len()].copy_from_slice(data);
        overlay.insert(path.clone(), content);
        debug!("[WRITE] wrote {} bytes", data.len());
        reply.written(data.len() as u32);
    } else {
        debug!("[WRITE] inode not found");
        reply.error(libc::ENOENT);
    }
}
