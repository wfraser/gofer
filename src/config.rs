use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server_address: String,
    pub document_root: PathBuf,
    pub hostname: String,
    pub port: u16
}
