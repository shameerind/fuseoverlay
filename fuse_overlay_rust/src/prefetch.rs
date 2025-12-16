use anyhow::Result;
use git2::{ObjectType, Repository};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::path::PathBuf;
use std::thread;
use crate::metrics::{debug, Metrics};
use crate::cache::LruCache;

#[allow(dead_code)]
pub fn fetch_blob_from_git(repo: &Repository, path: &Path) -> Result<Vec<u8>, git2::Error> {
    let head = repo.head()?.peel_to_commit()?;
    let mut tree = head.tree()?;
    for comp in path.iter() {
        let comp_str = comp.to_str().ok_or_else(|| git2::Error::from_str("invalid UTF-8 in path"))?;
        let entry_kind = tree.get_name(comp_str)
            .ok_or_else(|| git2::Error::from_str("path not found"))?
            .kind();
        
        if entry_kind == Some(ObjectType::Tree) {
            let next_tree = tree.get_name(comp_str)
                .ok_or_else(|| git2::Error::from_str("path not found"))?
                .to_object(repo)?
                .peel_to_tree()?;
            tree = next_tree;
        } else {
            return Ok(tree.get_name(comp_str)
                .ok_or_else(|| git2::Error::from_str("path not found"))?
                .to_object(repo)?
                .peel_to_blob()?
                .content()
                .to_vec());
        }
    }
    Ok(Vec::new())
}

#[allow(dead_code)]
pub fn prefetch_files(
    repo_path: PathBuf,
    paths: Vec<PathBuf>,
    overlay: Arc<LruCache>,
    metrics: Arc<Metrics>,
) {
    thread::spawn(move || {
        let Ok(repo) = Repository::open(&repo_path) else { return; };
        
        for path in paths {
            if overlay.contains_key(&path) {
                continue;
            }

            if let Ok(blob) = fetch_blob_from_git(&repo, &path) {
                debug!("[PREFETCH] Cached blob for {:?} ({} bytes)", path, blob.len());
                metrics.prefetch_count.fetch_add(1, Ordering::Relaxed);
                metrics.prefetch_bytes.fetch_add(blob.len() as u64, Ordering::Relaxed);
                overlay.insert(path.clone(), blob);
            }
        }
    });
}

pub fn prefetch_directory(
    repo_path: PathBuf,
    dir_path: PathBuf,
    head: git2::Oid,
    overlay: Arc<LruCache>,
    metrics: Arc<Metrics>,
) {
    thread::spawn(move || {
        let Ok(repo) = Repository::open(&repo_path) else { return; };
        let Ok(commit) = repo.find_commit(head) else { return; };
        let Ok(mut tree) = commit.tree() else { return; };
        
        for comp in dir_path.iter() {
            let Some(comp_str) = comp.to_str() else { return; };
            let next_tree = tree.get_name(comp_str)
                .and_then(|entry| entry.to_object(&repo).ok())
                .and_then(|obj| obj.peel_to_tree().ok());
            let Some(next) = next_tree else { return; };
            tree = next;
        }
        
        debug!("[PREFETCH] Prefetching directory: {:?}", dir_path);
        for entry in tree.iter() {
            if entry.kind() != Some(ObjectType::Blob) {
                continue;
            }
            
            let Some(name) = entry.name() else { continue; };
            let file_path = dir_path.join(name);
            
            if overlay.contains_key(&file_path) {
                continue;
            }
            
            if let Ok(obj) = entry.to_object(&repo) {
                if let Ok(blob) = obj.peel_to_blob() {
                    let content = blob.content().to_vec();
                    debug!("[PREFETCH] Cached {:?} ({} bytes)", file_path, content.len());
                    metrics.prefetch_count.fetch_add(1, Ordering::Relaxed);
                    metrics.prefetch_bytes.fetch_add(content.len() as u64, Ordering::Relaxed);
                    overlay.insert(file_path, content);
                }
            }
        }
        
        metrics.log();
    });
}
