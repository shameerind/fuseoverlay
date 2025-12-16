use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("GITFS_DEBUG").is_ok() {
            eprintln!($($arg)*);
        }
    };
}

pub(crate) use debug;

#[derive(Default)]
pub struct Metrics {
    pub prefetch_count: AtomicU64,
    pub prefetch_bytes: AtomicU64,
    pub on_demand_count: AtomicU64,
    pub on_demand_bytes: AtomicU64,
}

impl Metrics {
    pub fn log(&self) {
        let prefetch_cnt = self.prefetch_count.load(Ordering::Relaxed);
        let prefetch_bytes = self.prefetch_bytes.load(Ordering::Relaxed);
        let on_demand_cnt = self.on_demand_count.load(Ordering::Relaxed);
        let on_demand_bytes = self.on_demand_bytes.load(Ordering::Relaxed);
        
        debug!("----- GitFS Metrics -----");
        debug!("Prefetch: {} files, {} bytes", prefetch_cnt, prefetch_bytes);
        debug!("On-demand: {} files, {} bytes", on_demand_cnt, on_demand_bytes);
        
        let total = prefetch_cnt + on_demand_cnt;
        if total > 0 {
            let prefetch_pct = (prefetch_cnt * 100) / total;
            debug!("Cache hit rate: {}%", prefetch_pct);
        }
    }
}
