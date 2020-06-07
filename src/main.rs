mod config;
mod request;
mod request_stream;
mod types;

use anyhow::{bail, Context, Result};
use crate::request::Request;
use crate::request_stream::RequestStream;
use tokio::io::AsyncWriteExt;

fn parse_args() -> Result<config::Config> {
    match std::env::args_os().nth(1) {
        Some(path) => {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read config file {:?}", path))?;
            let config = toml::from_str(&text)
                .with_context(|| format!("error parsing config file {:?}", path))?;
            Ok(config)
        }
        None => {
            bail!("usage: {} <path to config.toml>", std::env::args().next().unwrap());
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = parse_args()?;

    let mut incoming = RequestStream::bind(&config.server_address).await
        .with_context(|| format!("failed to bind to address {}", config.server_address))?;
    eprintln!("listening for connections at {}", config.server_address);

    loop {
        match incoming.next_request().await {
            (Ok(Request { selector }), mut tx) => {
                eprintln!("selector: {}", selector);        
                tx.write_all(b"iok!\r\ni").await?;
                tx.write_all(selector.as_bytes()).await?;
                tx.write_all(b"\r\n").await?;
            }
            (Err(e), mut tx) => {
                eprintln!("error: {:?}", e);
                tx.write_all(b"3Request Error\r\n3").await?;
                tx.write_all(format!("{:?}", e).as_bytes()).await?;
            }
        }
    }
}
