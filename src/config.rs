use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct BucketConfig {
    pub name: String,
    pub storage: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub port: u16,
    pub endpoint: String,
    pub verbose: bool,
    #[serde(default = "default_cache_size")]
    pub cache_size: usize,
    pub auth: Option<AuthConfig>,
    pub buckets: Vec<BucketConfig>,
}

fn default_cache_size() -> usize {
    10000
}
