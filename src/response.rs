use bytes::BytesMut;
use crate::types::ItemType;
use futures::sink::SinkExt;
use futures::stream::{self, StreamExt};
use tokio::fs::File;
use tokio::io::{self, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::{Encoder, FramedWrite};

#[derive(Debug)]
pub enum Response {
    Menu(Menu),
    File(File),
    Error(String),
}

#[derive(Debug)]
pub struct MenuItem {
    pub typ: ItemType,
    pub text: String,
    pub selector: String,
    pub host: Option<String>,
    pub port: Option<String>,
}

impl MenuItem {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            typ: ItemType::Info,
            text: text.into(),
            selector: String::new(),
            host: None,
            port: None,
        }
    }

    pub fn new(typ: ItemType, text: impl Into<String>, selector: impl Into<String>, host: impl Into<String>, port: impl Into<String>) -> Self {
        Self {
            typ,
            text: text.into(),
            selector: selector.into(),
            host: Some(host.into()),
            port: Some(port.into()),
        }
    }
}

#[derive(Debug)]
pub struct Menu {
    pub items: Vec<MenuItem>,
}

impl Response {
    pub async fn write<W: AsyncWrite + Unpin>(&mut self, mut w: W) -> Result<(), io::Error> {
        match self {
            Response::Menu(menu) => {
                FramedWrite::new(&mut w, MenuItemEncoder)
                    .send_all(&mut stream::iter(&menu.items).map(Ok))
                    .await?;
                w.write_all(b".\r\n").await?;
            }
            Response::File(f) => {
                io::copy(f, &mut w).await?;
            }
            Response::Error(msg) => {
                w.write_all(&[ItemType::Error.into_u8()]).await?;
                w.write_all(msg.as_bytes()).await?;
                w.write_all(b"\terror\terror.host\t1\r\n.\r\n").await?;
            }
        }
        Ok(())
    }
}

struct MenuItemEncoder;

impl Encoder<&MenuItem> for MenuItemEncoder {
    type Error = io::Error;

    fn encode(&mut self, item: &MenuItem, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(&[item.typ.into_u8()]);
        dst.extend_from_slice(item.text.as_bytes());
        dst.extend_from_slice(b"\t");
        dst.extend_from_slice(item.selector.as_bytes());
        dst.extend_from_slice(b"\t");
        dst.extend_from_slice(item.host.as_ref().map(String::as_bytes).unwrap_or(b"error.host"));
        dst.extend_from_slice(b"\t");
        dst.extend_from_slice(item.port.as_ref().map(String::as_bytes).unwrap_or(b"1"));
        dst.extend_from_slice(b"\r\n");
        Ok(())
    }
}
