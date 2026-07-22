use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub port: u16,
    pub index_path: String,
    pub log_level: String,
    pub max_results: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 3000,
            index_path: ".graphswarm/index.db".into(),
            log_level: "info".into(),
            max_results: 100,
        }
    }
}
