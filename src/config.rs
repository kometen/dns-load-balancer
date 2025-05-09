use anyhow::{Ok, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Config {
    pub servers: Vec<ServerConfig>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ServerConfig {
    pub address: String,
    pub use_tls: bool,
    pub description: String,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = fs::read_to_string(path.as_ref())?;

        match path.as_ref().extension().and_then(|ext| ext.to_str()) {
            Some("toml") => {
                let config: Config = toml::from_str(&contents)?;
                Ok(config)
            }

            _ => Err(anyhow::anyhow!("Unsupported file format")),
        }
    }
}

//pub const LOCALHOST_PORT_V4: &str = "127.0.0.1:5353";
//pub const LOCALHOST_PORT_V6: &str = "[::1]:5353";
pub const CACHE_TTL: u64 = 300; // 5 minutes
pub const DNS_TIMEOUT: u64 = 3; // seconds
pub const KUBERNETES_DOMAIN: &str = "cluster.local.";
