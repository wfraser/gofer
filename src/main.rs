mod config;
mod request;
mod request_stream;
mod response;
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
        use response::{Response, Menu, MenuItem};
        use types::ItemType;
        let (req, tx) = incoming.next_request().await;
        let mut response = match req {
            Ok(Request { selector }) => {
                eprintln!("selector: {}", selector);
                Response::Menu(Menu { items: vec![
                    MenuItem::info("ok!"),
                    MenuItem::info(selector),
                    MenuItem::new(ItemType::File, "cool file", "/foobar.txt", "localhost", "7070"),
                ]})
            }
            Err(e) => {
                eprintln!("error: {:?}", e);
                Response::Error(format!("Request error: {:?}", e))
            }
        };
        if let Err(e) = response.write(tx).await {
            eprintln!("error writing response: {}", e);
        }
    }
}
