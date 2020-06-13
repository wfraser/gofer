mod config;
mod fs;
mod menu;
mod request;
mod request_stream;
mod response;
mod types;

use anyhow::{bail, Context, Result};
use crate::config::Config;
use crate::fs::{DirEntry, FileType};
use crate::menu::{Menu, MenuItem, MenuItemDecoder};
use crate::request::Request;
use crate::request_stream::RequestStream;
use crate::response::Response;
use crate::types::ItemType;
use futures::future;
use futures::stream::{self, StreamExt};
use std::path::Path;
use std::rc::Rc;
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
    } else if req.selector.starts_with("GET ")
        && (req.selector.ends_with(" HTTP/1.1") || req.selector.ends_with(" HTTP/1.0"))
    {
        // We don't know what the type is, but let's assume directory.
        let url = format!("gopher://{}:{}/1{}",
            config.hostname,
            config.port,
            &req.selector[4 .. req.selector.len() - 9],
        );
        return Response::Raw(http_response(&url).into_bytes());
    } else if req.selector.starts_with('/') {
        if req.selector == "/.." || req.selector.contains("/../") || req.selector.contains("//") {
            return Response::Error("directory traversal denied".into());
        }
        config.document_root.join(&req.selector[1..])
    } else {
        return Response::Error("not found".into());
    };

    match fs::lookup(&path).await {
        Ok(FileType::Menu { file: menu_file, path: menu_path }) => {
            eprintln!("menu {:?}", menu_path);
            let config_rc = Rc::new(config.to_owned());
            let items = FramedRead::new(menu_file, MenuItemDecoder)
                .enumerate()
                .filter_map(move |(line, result)| future::ready(
                    match result {
                        Ok(x) => Some(x),
                        Err(e) => {
                            eprintln!("error in {:?} on line {}: {}",
                                menu_path,
                                line + 1,
                                e);
                            None
                        }
                    }))
                .map(move |mut item| {
                    if item.typ != ItemType::Info && item.typ != ItemType::Error {
                        if item.port.is_none() {
                            if item.host.is_none() {
                                item.host = Some(config_rc.hostname.clone());
                                item.port = Some(config_rc.port.to_string());
                            } else {
                                item.port = Some("70".to_owned());
                            }
                        } else if item.host.is_none() {
                            item.host = Some(config_rc.hostname.clone());
                        }
                    }
                    item
                });
            Response::Menu(Menu::new(items))
        }
        Ok(FileType::Directory) => {
            eprintln!("directory {:?}", path);
            generate_menu(&path, &req.selector, config).await
        }
        Ok(FileType::File(file)) => {
            eprintln!("file {:?}", path);
            Response::File(file)
        }
        Ok(FileType::NotFound) => {
            eprintln!("not found {:?}", path);
            Response::Error("not found".into())
        }
        Err(e) => e.into(),
    }
}

async fn direntry_menuitem(entry: DirEntry, selector: Rc<String>, config: Rc<Config>)
    -> Option<MenuItem>
{
    async fn inner(entry: DirEntry, selector: &str, config: &Config) -> Option<MenuItem> {
        let is_dir = match entry.file_type()
            .await
            .map(|ft| ft.is_dir())
        {
            Ok(b) => b,
            Err(e) => {
                eprintln!("error getting file type of {:?}: {}", entry.path(), e);
                return None;
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
        Some(MenuItem::new(
            typ,
            text,
            selector,
            config.hostname.clone(),
            config.port.to_string()))
    }
    inner(entry, &selector, &config).await
}


async fn generate_menu(path: &Path, selector: &str, config: &Config) -> Response {
    match fs::read_dir(path).await {
        Ok(stream) => {
            let header = stream::iter(vec![
                MenuItem::info(format!("[{}{}]", &config.hostname, selector)),
                MenuItem::info("")
            ]);

            let selector_rc = Rc::new(selector.to_owned());
            let config_rc = Rc::new(config.to_owned());
            let items = stream
                .filter_map(|result| future::ready(result.ok()))
                .filter_map(move |entry| {
                    direntry_menuitem(entry, selector_rc.clone(), config_rc.clone())
                });

            Response::Menu(Menu::new(header.chain(items)))
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

fn http_response(url: &str) -> String {
    // This isn't really valid HTTP because it's missing required headers, but it's enough to get
    // the page to display in a browser.
    format!("HTTP/1.0 400 Bad Request\r
Content-Type: text/html\r
\r
<!doctype html>
<html>
    <head>
        <title>This is a Gopher server</title>
    </head>
    <body>
        <p>This is a Gopher server but it looks like you've made a HTTP request.
        <p>If you're using a Gopher-capable browser, click <a href=\"{0}\">here</a> to use a Gopher
           URL to view this page properly.
        <address>generated by gofer</address>
    </body>
</html>",
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
