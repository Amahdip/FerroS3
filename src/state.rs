use std::collections::HashMap;
use std::path::PathBuf;
use dashmap::DashMap;
use crate::config::Config;
use crate::cache::CachedStat;

pub struct AppState {
    pub config: Config,
    pub cache: DashMap<String, CachedStat>,
    pub storage_map: HashMap<String, PathBuf>,
}
