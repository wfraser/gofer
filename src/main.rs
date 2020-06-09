mod config;
mod request;
mod request_stream;
mod response;
mod types;

use anyhow::{bail, Context, Result};
use crate::config::Config;
use crate::request::Request;
use crate::request_stream::RequestStream;
use crate::response::{Response, Menu, MenuItem, MenuItemDecoder};
use crate::types::ItemType;
use futures::stream::StreamExt;
use std::path::Path;
use tokio::fs::{self, File};
use tokio::io;
use tokio_util::codec::FramedRead;

fn parse_args() -> Result<Config> {
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

async fn handle_request(config: &Config, req: Request) -> Response {
    let path = if req.selector.is_empty() {
        config.document_root.clone()
    } else if req.selector.starts_with("URL:") {
        return Response::Raw(html_redirect(&req.selector[4..]).into_bytes());
    } else if req.selector.starts_with('/') {
        if req.selector == "/.." || req.selector.contains("/../") {
            return Response::Error("directory traversal denied".into());
        }
        config.document_root.join(&req.selector[1..])
    } else {
        return Response::Error("not found".into());
    };

    eprintln!("looking up {:?}", path);
    let meta = match fs::metadata(&path).await {
        Ok(meta) => meta,
        Err(e) => return e.into(),
    };

    if meta.is_dir() {
        let menu_path = path.join("!menu");
        match File::open(&menu_path).await {
            Ok(mut menu_file) => {
                let mut menu = Menu { items: vec![] };
                let mut framed = FramedRead::new(&mut menu_file, MenuItemDecoder::new());
                let mut line = 0u32;
                while let Some(item_result) = framed.next().await {
                    line += 1;
                    match item_result {
                        Ok(mut item) => {
                            if item.typ != ItemType::Info && item.typ != ItemType::Error {
                                if item.host.is_none() {
                                    item.host = Some(config.hostname.clone());
                                }
                                if item.port.is_none() {
                                    item.port = Some(config.port.to_string());
                                }
                            }
                            menu.items.push(item);
                        }
                        Err(e) => {
                            eprintln!("error in {:?} on line {}: {}",
                                menu_path,
                                line,
                                e);
                        }
                    }
                }
                Response::Menu(menu)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                generate_menu(config, &path, &req.selector).await
            }
            Err(e) => {
                e.into()
            }
        }
    } else {
        match tokio::fs::File::open(&path).await {
            Ok(file) => Response::File(file),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Response::Error("not found".into()),
            Err(e) => e.into(),
        }
    }
}

async fn generate_menu(config: &Config, path: &Path, selector: &str) -> Response {
    match fs::read_dir(path).await {
        Ok(mut stream) => {
            let mut menu = Menu { items: vec![
                MenuItem::info(format!("[{}{}]", &config.hostname, selector)),
                MenuItem::info(""),
            ]};
            while let Some(entry_result) = stream.next().await {
                match entry_result {
                    Ok(entry) => {
                        let is_dir = match entry.file_type()
                            .await
                            .map(|ft| ft.is_dir())
                        {
                            Ok(b) => b,
                            Err(e) => {
                                eprintln!("error getting file type of {:?}: {}", entry.path(), e);
                                continue;
                            }
                        };

                        // TODO: if it's not representable as UTF-8, this will be bad.
                        let text = entry.file_name().to_string_lossy().into_owned();
                        let selector = selector.to_owned() + "/" + &text;
                        let typ = if is_dir {
                            ItemType::Directory
                        } else {
                            // TODO: file types for images, audio, etc. based on extensions.
                            ItemType::File
                        };
                        menu.items.push(
                            MenuItem::new(
                                typ,
                                text,
                                selector,
                                config.hostname.clone(),
                                config.port.to_string()));
                    }
                    Err(e) => {
                        eprintln!("error iterating directory {:?}: {}", path, e);
                        continue;
                    }
                }
            }
            Response::Menu(menu)
        }
        Err(e) => e.into(),
    }
}

/// For clients that don't understand the "URL:..." selector format.
fn html_redirect(url: &str) -> String {
    format!(r#"<!doctype html>
<html>
    <head>
        <meta http-equiv="refresh" content="5;URL={0}">
        <title>Gopher redirect to URL: {0}</title>
    </head>
    <body>
        <p>You're being redirected to a HTTP URL: <code>{0}</code>
        <p>Click <a href="{0}">here</a> if you are not redirected automatically.
        <address>generated by gofer</address>
    </body>
</html>"#,
    url)
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = parse_args()?;

    let mut incoming = RequestStream::bind(&config.server_address).await
        .with_context(|| format!("failed to bind to address {}", config.server_address))?;
    eprintln!("listening for connections at {}", config.server_address);

    loop {
        let (req, tx) = incoming.next_request().await;
        let mut response = match req {
            Ok(req) => {
                eprintln!("selector: {}", req.selector);
                handle_request(&config, req).await
            }
            Err(e) => {
                eprintln!("error: {:?}", e);
                Response::Error(format!("Bad request: {:?}", e))
            }
        };
        if let Err(e) = response.write(tx).await {
            eprintln!("error writing response: {}", e);
        }
    }
}
