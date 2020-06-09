use bytes::{Buf, BytesMut};
use crate::types::ItemType;
use futures::sink::SinkExt;
use futures::stream::{self, StreamExt};
use thiserror::Error;
use tokio::fs::File;
use tokio::io::{self, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::{Decoder, Encoder, FramedWrite};

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

struct MenuItemDecoder {
    partial: Option<
        (ItemType, Option<
            (String /* text */, Option<
                (String /* selector */, Option<
                    (String /* host */, Option<
                        String /* port */>)>)>)>)>,
    next_index: usize,
}

impl MenuItemDecoder {
    pub fn new() -> Self {
        Self {
            partial: None,
            next_index: 0,
        }
    }
}

#[derive(Error, Debug)]
pub enum MenuItemParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid UTF-8 string")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("{0}")]
    Message(String),
}

impl Decoder for MenuItemDecoder {
    type Item = MenuItem;
    type Error = MenuItemParseError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // this implementation is probably kind of overkill :P

        // Read a utf8 string ended by \t, \r, or \n.
        fn next_string(next_index: &mut usize, buf: &BytesMut)
            -> Result<Option<String>, MenuItemParseError>
        {
            match buf[*next_index..]
                .iter()
                .cloned()
                .position(|c| c == b'\t' || c == b'\r' || c == b'\n')
            {
                Some(off) => {
                    let text_bytes = &buf[*next_index .. *next_index + off];
                    let text = std::str::from_utf8(text_bytes)
                        .map_err(MenuItemParseError::Utf8)?
                        .to_owned();
                    *next_index += off + 1;
                    Ok(Some(text))
                }
                None => Ok(None),
            }
        }

        loop {
            if self.next_index >= buf.len() {
                // We need look-ahead of 1 to detect CR-LF.
                return Ok(None);
            }

            let next = match self.partial.take() {
                None => {
                    assert_eq!(0, self.next_index);
                    if buf.len() >= 2 && &buf[0..2] == b"\r\n" {
                        // special case
                        buf.advance(2);
                        return Ok(Some(MenuItem {
                            typ: ItemType::Info,
                            text: String::new(),
                            selector: String::new(),
                            host: None,
                            port: None,
                        }));
                    }
                    let typ = match buf[0] {
                        b'\r' | b'\n' | b'\t' => {
                            let msg = format!("invalid item type {:?}", buf[0]);
                            return Err(MenuItemParseError::Message(msg));
                        }
                        byte => ItemType::from_u8(byte),
                    };
                    self.next_index = 1;
                    Some((typ, None))
                }
                Some((typ, None)) => {
                    match next_string(&mut self.next_index, buf) {
                        Ok(Some(text)) => {
                            Some((typ, Some((text, None))))
                        }
                        Ok(None) => return Ok(None),
                        Err(e) => return Err(e),
                    }
                },
                Some((typ, Some((text, None)))) => {
                    if &buf[self.next_index - 1 .. self.next_index + 1] == b"\r\n" {
                        // End of line, using empty selector, host & port
                        buf.advance(self.next_index + 1);
                        return Ok(Some(MenuItem {
                            typ,
                            text,
                            selector: String::new(),
                            host: None,
                            port: None,
                        }));
                    }
                    match next_string(&mut self.next_index, buf) {
                        Ok(Some(selector)) => {
                            Some((typ, Some((text, Some((selector, None))))))
                        }
                        Ok(None) => return Ok(None),
                        Err(e) => return Err(e),
                    }
                },
                Some((typ, Some((text, Some((selector, None)))))) => {
                    if &buf[self.next_index - 1 .. self.next_index + 1] == b"\r\n" {
                        // End of line, using empty host & port
                        buf.advance(self.next_index + 1);
                        return Ok(Some(MenuItem {
                            typ,
                            text,
                            selector,
                            host: None,
                            port: None,
                        }));
                    }
                    match next_string(&mut self.next_index, buf) {
                        Ok(Some(host)) => {
                            Some((typ, Some((text, Some((selector, Some((host, None))))))))
                        }
                        Ok(None) => return Ok(None),
                        Err(e) => return Err(e),
                    }
                },
                Some((typ, Some((text, Some((selector, Some((host, None)))))))) => {
                    match next_string(&mut self.next_index, buf) {
                        Ok(Some(port)) => {
                            Some((typ, Some((text, Some((selector, Some((host, Some(port)))))))))
                        }
                        Ok(None) => return Ok(None),
                        Err(e) => return Err(e),
                    }
                }
                Some((typ, Some((text, Some((selector, Some((host, Some(port))))))))) => {
                    if &buf[self.next_index - 1 .. self.next_index + 1] == b"\r\n" {
                        buf.advance(self.next_index + 1);
                        self.next_index = 0;
                        return Ok(Some(MenuItem {
                            typ,
                            text,
                            selector,
                            host: Some(host),
                            port: Some(port),
                        }));
                    } else {
                        let msg = format!("extra garbage at end of line: {:?}",
                            std::str::from_utf8(&buf[self.next_index - 1 ..]));
                        return Err(MenuItemParseError::Message(msg));
                    }
                }
            };
            self.partial = next;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_menuitem() {
        let mut decoder = MenuItemDecoder::new();
        let mut buf = BytesMut::from("1text\tselector\thost\tport\r\n");
        let item = decoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Directory, item.typ);
        assert_eq!("text", item.text);
        assert_eq!("selector", item.selector);
        assert_eq!(Some("host"), item.host.as_deref());
        assert_eq!(Some("port"), item.port.as_deref());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_menuitem_incomplete() {
        let mut decoder = MenuItemDecoder::new();
        let mut buf = BytesMut::from("1text\tselector\r\n");
        let item = decoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Directory, item.typ);
        assert_eq!("text", item.text);
        assert_eq!("selector", item.selector);
        assert_eq!(None, item.host);
        assert_eq!(None, item.port);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_info_short() {
        let mut decoder = MenuItemDecoder::new();
        let mut buf = BytesMut::from("itext\r\n");
        let item = decoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Info, item.typ);
        assert_eq!("text", item.text);
        assert_eq!("", item.selector);
        assert_eq!(None, item.host);
        assert_eq!(None, item.port);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_info_only_line() {
        let mut decoder = MenuItemDecoder::new();
        let mut buf = BytesMut::from("i\r\n");
        let item = decoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Info, item.typ);
        assert_eq!("", item.text);
        assert_eq!("", item.selector);
        assert_eq!(None, item.host);
        assert_eq!(None, item.port);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_only_newline() {
        let mut decoder = MenuItemDecoder::new();
        let mut buf = BytesMut::from("\r\n");
        let item = decoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Info, item.typ);
        assert_eq!("", item.text);
        assert_eq!("", item.selector);
        assert_eq!(None, item.host);
        assert_eq!(None, item.port);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_bad_type() {
        let mut decoder = MenuItemDecoder::new();
        let mut buf = BytesMut::from("\t\r\n");
        match decoder.decode(&mut buf) {
            Err(MenuItemParseError::Message(_)) => (),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn test_parse_extra_garbage() {
        let mut decoder = MenuItemDecoder::new();
        let mut buf = BytesMut::from("itext\tselector\thost\tport\tspaghetti");
        match decoder.decode(&mut buf) {
            Err(MenuItemParseError::Message(_)) => (),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn test_parse_truncated() {
        let mut decoder = MenuItemDecoder::new();
        let mut buf = BytesMut::from("itext\tselector\thost\tport"); // missing CR-LF
        match decoder.decode(&mut buf) {
            Ok(None) => (),
            other => panic!("unexpected {:?}", other),
        }
    }
}
