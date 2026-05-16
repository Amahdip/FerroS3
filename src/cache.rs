use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct CachedStat {
    pub size: u64,
    pub mod_time: DateTime<Utc>,
    pub etag: String,
}
