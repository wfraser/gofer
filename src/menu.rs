use bytes::{Buf, BytesMut};
use crate::types::ItemType;
use futures::stream::Stream;
use std::pin::Pin;
use thiserror::Error;
use tokio::io;
use tokio_util::codec::{Decoder, Encoder};

pub struct Menu {
    pub items: Pin<Box<dyn Stream<Item = MenuItem>>>,
}

impl Menu {
    pub fn new<S: Stream<Item = MenuItem> + 'static>(s: S) -> Self {
        Self {
            items: Box::pin(s),
        }
    }
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

pub struct MenuItemEncoder;

impl Encoder<MenuItem> for MenuItemEncoder {
    type Error = io::Error;

    fn encode(&mut self, item: MenuItem, dst: &mut BytesMut) -> Result<(), Self::Error> {
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

pub struct MenuItemDecoder;

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
        let mut line = {
            match buf.iter().position(|c| *c == b'\n') {
                Some(idx) => buf.split_to(idx + 1),
                None => {
                    // We need at least a whole line.
                    return Ok(None);
                }
            }
        };

        if line.ends_with(b"\r\n") {
            line.truncate(line.len() - 2);
        } else {
            assert!(line.ends_with(b"\n"));
            line.truncate(line.len() - 1);
        }

        fn next_field(buf: &mut BytesMut) -> BytesMut {
            match buf.iter().position(|c| *c == b'\t') {
                Some(idx) => {
                    let field = buf.split_to(idx);
                    buf.advance(1); // Skip the tab.
                    field
                }
                None => {
                    // Take the entire buffer.
                    buf.split()
                }
            }
        }

        fn next_string(buf: &mut BytesMut) -> Result<String, MenuItemParseError> {
            Ok(std::str::from_utf8(&next_field(buf))?.to_owned())
        }

        if line.is_empty() {
            return Ok(Some(MenuItem {
                typ: ItemType::Info,
                text: String::new(),
                selector: String::new(),
                host: None,
                port: None,
            }));
        }

        let typ = match line[0] {
            0 ..= 0x20 => {
                // disallow unprintable characters
                let msg = format!("invalid item type {:?}", char::from(line[0]));
                return Err(MenuItemParseError::Message(msg));
            }
            byte => ItemType::from_u8(byte),
        };
        line.advance(1);

        let text = next_string(&mut line)?;

        if line.is_empty() {
            return Ok(Some(MenuItem {
                typ,
                text,
                selector: String::new(),
                host: None,
                port: None,
            }));
        }

        let selector = next_string(&mut line)?;

        if line.is_empty() {
            return Ok(Some(MenuItem {
                typ,
                text,
                selector,
                host: None,
                port: None,
            }));
        }

        let host = next_string(&mut line)?;

        if line.is_empty() {
            return Ok(Some(MenuItem {
                typ,
                text,
                selector,
                host: Some(host),
                port: None,
            }));
        }

        let port = next_string(&mut line)?;

        if line.is_empty() {
            return Ok(Some(MenuItem {
                typ,
                text,
                selector,
                host: Some(host),
                port: Some(port),
            }));
        }

        let msg = format!("extra garbage at end of line: {:?}",
            std::str::from_utf8(&line));
        Err(MenuItemParseError::Message(msg))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_menuitem() {
        let mut buf = BytesMut::from("1text\tselector\thost\tport\r\n");
        let item = MenuItemDecoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Directory, item.typ);
        assert_eq!("text", item.text);
        assert_eq!("selector", item.selector);
        assert_eq!(Some("host"), item.host.as_deref());
        assert_eq!(Some("port"), item.port.as_deref());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_menuitem_incomplete() {
        let mut buf = BytesMut::from("1text\tselector\r\n");
        let item = MenuItemDecoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Directory, item.typ);
        assert_eq!("text", item.text);
        assert_eq!("selector", item.selector);
        assert_eq!(None, item.host);
        assert_eq!(None, item.port);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_info_short() {
        let mut buf = BytesMut::from("itext\r\n");
        let item = MenuItemDecoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Info, item.typ);
        assert_eq!("text", item.text);
        assert_eq!("", item.selector);
        assert_eq!(None, item.host);
        assert_eq!(None, item.port);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_info_only_line() {
        let mut buf = BytesMut::from("i\r\n");
        let item = MenuItemDecoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Info, item.typ);
        assert_eq!("", item.text);
        assert_eq!("", item.selector);
        assert_eq!(None, item.host);
        assert_eq!(None, item.port);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_only_newline() {
        let mut buf = BytesMut::from("\r\n");
        let item = MenuItemDecoder.decode(&mut buf).unwrap().unwrap();
        assert_eq!(ItemType::Info, item.typ);
        assert_eq!("", item.text);
        assert_eq!("", item.selector);
        assert_eq!(None, item.host);
        assert_eq!(None, item.port);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_parse_bad_type() {
        let mut buf = BytesMut::from("\t\r\n");
        match MenuItemDecoder.decode(&mut buf) {
            Err(MenuItemParseError::Message(_)) => (),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn test_parse_extra_garbage() {
        let mut buf = BytesMut::from("itext\tselector\thost\tport\tspaghetti\r\n");
        match MenuItemDecoder.decode(&mut buf) {
            Err(MenuItemParseError::Message(_)) => (),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn test_parse_truncated() {
        let mut buf = BytesMut::from("itext\tselector\thost\tport"); // missing CR-LF
        match MenuItemDecoder.decode(&mut buf) {
            Ok(None) => (),
            other => panic!("unexpected {:?}", other),
        }
    }
}
