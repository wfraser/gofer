use crate::menu::{Menu, MenuItemEncoder};
use crate::types::ItemType;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use tokio::fs::File;
use tokio::io::{self, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::FramedWrite;

pub enum Response {
    Menu(Menu),
    File(File),
    Raw(Vec<u8>),
    Error(String),
}

impl From<io::Error> for Response {
    fn from(e: io::Error) -> Response {
        eprintln!("I/O error: {e}");
        // Don't leak details of the error to clients.
        Response::Error("I/O error".to_owned())
    }
}

impl Response {
    pub async fn write<W: AsyncWrite + Unpin>(&mut self, mut w: W) -> Result<(), io::Error> {
        match self {
            Response::Menu(menu) => {
                FramedWrite::new(&mut w, MenuItemEncoder)
                    .send_all(&mut menu.items.by_ref().map(Ok))
                    .await?;
                w.write_all(b".\r\n").await?;
            }
            Response::File(f) => {
                io::copy(f, &mut w).await?;
            }
            Response::Raw(bytes) => {
                io::copy(&mut std::io::Cursor::new(bytes), &mut w).await?;
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
