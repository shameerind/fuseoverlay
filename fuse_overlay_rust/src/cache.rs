use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;

/// LRU cache for file contents with size limits
pub struct LruCache {
    data: Mutex<LruCacheInner>,
}

struct LruCacheInner {
    cache: HashMap<PathBuf, Vec<u8>>,
    access_order: VecDeque<PathBuf>,
    current_size: usize,
    max_size: usize,      // Maximum total bytes
    max_entries: usize,   // Maximum number of entries
}

impl LruCache {
    pub fn new(max_size: usize, max_entries: usize) -> Self {
        Self {
            data: Mutex::new(LruCacheInner {
                cache: HashMap::new(),
                access_order: VecDeque::new(),
                current_size: 0,
                max_size,
                max_entries,
            }),
        }
    }

    pub fn get(&self, path: &PathBuf) -> Option<Vec<u8>> {
        let mut inner = self.data.lock().unwrap();
        
        // Clone data first to avoid borrow checker issues
        let result = inner.cache.get(path).cloned();
        
        if result.is_some() {
            // Move to front (most recently used)
            if let Some(pos) = inner.access_order.iter().position(|p| p == path) {
                inner.access_order.remove(pos);
            }
            inner.access_order.push_front(path.clone());
        }
        
        result
    }

    pub fn insert(&self, path: PathBuf, data: Vec<u8>) {
        let mut inner = self.data.lock().unwrap();
        let data_size = data.len();
        
        // Remove old entry if exists
        if let Some(old_data) = inner.cache.remove(&path) {
            inner.current_size -= old_data.len();
            if let Some(pos) = inner.access_order.iter().position(|p| p == &path) {
                inner.access_order.remove(pos);
            }
        }
        
        // Evict until we have space
        while (inner.current_size + data_size > inner.max_size 
               || inner.cache.len() >= inner.max_entries)
               && !inner.cache.is_empty() {
            if let Some(old_path) = inner.access_order.pop_back() {
                if let Some(old_data) = inner.cache.remove(&old_path) {
                    inner.current_size -= old_data.len();
                }
            }
        }
        
        // Insert new entry
        inner.cache.insert(path.clone(), data);
        inner.access_order.push_front(path);
        inner.current_size += data_size;
    }

    pub fn remove(&self, path: &PathBuf) -> Option<Vec<u8>> {
        let mut inner = self.data.lock().unwrap();
        
        if let Some(data) = inner.cache.remove(path) {
            inner.current_size -= data.len();
            if let Some(pos) = inner.access_order.iter().position(|p| p == path) {
                inner.access_order.remove(pos);
            }
            Some(data)
        } else {
            None
        }
    }

    pub fn contains_key(&self, path: &PathBuf) -> bool {
        self.data.lock().unwrap().cache.contains_key(path)
    }

    #[allow(dead_code)]
    pub fn clear(&self) {
        let mut inner = self.data.lock().unwrap();
        inner.cache.clear();
        inner.access_order.clear();
        inner.current_size = 0;
    }

    #[allow(dead_code)]
    pub fn stats(&self) -> CacheStats {
        let inner = self.data.lock().unwrap();
        CacheStats {
            entries: inner.cache.len(),
            total_bytes: inner.current_size,
            max_bytes: inner.max_size,
            max_entries: inner.max_entries,
        }
    }

    /// Iterate over all entries (for operations like readdir)
    pub fn iter<F>(&self, mut f: F) 
    where
        F: FnMut(&PathBuf, &Vec<u8>),
    {
        let inner = self.data.lock().unwrap();
        for (path, data) in inner.cache.iter() {
            f(path, data);
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entries: usize,
    pub total_bytes: usize,
    pub max_bytes: usize,
    pub max_entries: usize,
}

#[allow(dead_code)]
impl CacheStats {
    pub fn usage_percent(&self) -> f64 {
        (self.total_bytes as f64 / self.max_bytes as f64) * 100.0
    }
}
