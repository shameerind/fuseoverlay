use fuser::{FileType, ReplyDirectory};
use git2::{ObjectType, Repository};
use std::path::PathBuf;
use std::sync::Arc;
use crate::metrics::debug;
use crate::node_cache::NodeCache;
use crate::types::{Node, ROOT_INO, i32_to_filemode};
use crate::cache::LruCache;

pub fn read_directory(
    node: &Node,
    offset: i64,
    node_cache: &NodeCache,
    overlay: &Arc<LruCache>,
    repo: &Repository,
    head: git2::Oid,
    mut reply: ReplyDirectory,
) {
    debug!("[READDIR] ino={}, offset={}", node.ino, offset);
    debug!("[READDIR] path={:?}, kind={:?}", node.path, node.kind);
    
    if node.kind != FileType::Directory {
        debug!("[READDIR] not a directory");
        return reply.error(libc::ENOTDIR);
    }

    let mut entries: Vec<(u64, FileType, String)> = vec![];
    
    // Add . and ..
    entries.push((node.ino, FileType::Directory, ".".to_string()));
    
    let parent_ino = if node.path == PathBuf::new() {
        ROOT_INO
    } else {
        node.path.parent()
            .and_then(|p| node_cache.get_ino_by_path(p))
            .unwrap_or(ROOT_INO)
    };
    entries.push((parent_ino, FileType::Directory, "..".to_string()));

    // Git entries
    if let Ok(commit) = repo.find_commit(head) {
        if let Ok(mut curr_tree) = commit.tree() {
            let mut valid = true;
            for comp in node.path.iter() {
                if let Some(comp_str) = comp.to_str() {
                    let next_tree = curr_tree.get_name(comp_str)
                        .and_then(|entry| entry.to_object(repo).ok())
                        .and_then(|obj| obj.peel_to_tree().ok());
                    
                    if let Some(tree) = next_tree {
                        curr_tree = tree;
                    } else {
                        valid = false;
                        break;
                    }
                } else {
                    valid = false;
                    break;
                }
            }

            if valid {
                for e in curr_tree.iter() {
                    let kind = match e.kind() {
                        Some(ObjectType::Tree) => FileType::Directory,
                        Some(ObjectType::Blob) => FileType::RegularFile,
                        _ => continue,
                    };
                    let name = match e.name() {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    
                    let child_path = node.path.join(&name);
                    let child_ino = if let Some(ino) = node_cache.get_ino_by_path(&child_path) {
                        ino
                    } else {
                        let ino = node_cache.alloc_ino(&child_path);
                        let size = if kind == FileType::RegularFile {
                            e.to_object(repo).ok()
                                .and_then(|o| o.peel_to_blob().ok())
                                .map(|b| b.size() as u64)
                                .unwrap_or(0)
                        } else {
                            0
                        };
                        let child_node = Node {
                            ino,
                            kind,
                            size,
                            path: child_path.clone(),
                            git_mode: Some(i32_to_filemode(e.filemode())),
                        };
                        node_cache.insert_node(ino, child_node);
                        ino
                    };
                    
                    entries.push((child_ino, kind, name));
                }
            }
        }
    }

    // Overlay entries - collect them first
    let mut overlay_entries = Vec::new();
    overlay.iter(|p, data| {
        if p.parent() == Some(&node.path) {
            overlay_entries.push((p.clone(), data.clone()));
        }
    });
    
    for (p, data) in overlay_entries {
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        
        if entries.iter().any(|(_, _, n)| n == &name) {
            continue;
        }
        
        let (child_ino, kind) = if let Some(ino) = node_cache.get_ino_by_path(&p) {
            if let Some(existing_node) = node_cache.get_node(&ino) {
                (ino, existing_node.kind)
            } else {
                let child_node = Node {
                    ino,
                    kind: FileType::RegularFile,
                    size: data.len() as u64,
                    path: p.clone(),
                    git_mode: None,
                };
                node_cache.insert_node(ino, child_node);
                (ino, FileType::RegularFile)
            }
        } else {
            let ino = node_cache.alloc_ino(&p);
            let child_node = Node {
                ino,
                kind: FileType::RegularFile,
                size: data.len() as u64,
                path: p.clone(),
                git_mode: None,
            };
            node_cache.insert_node(ino, child_node);
            (ino, FileType::RegularFile)
        };
        
        entries.push((child_ino, kind, name));
    }    // Add entries starting from offset
    for (i, (ino, kind, name)) in entries.into_iter().enumerate().skip(offset as usize) {
        if reply.add(ino, (i + 1) as i64, kind, name) {
            break;
        }
    }
    
    reply.ok();
}
