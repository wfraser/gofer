use bytes::BytesMut;
use tokio::stream::StreamExt;
use thiserror::Error;
use tokio_util::codec::Decoder;

#[derive(Debug)]
pub struct Request {
    pub selector: String,
}

#[derive(Error, Debug)]
pub enum RequestError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid UTF-8 string")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("Request line too long")]
    TooLong,

    #[error("Invalid selector: {0}")]
    InvalidSelector(String),
}

pub struct RequestDecoder {
    max_length: usize,
    next_index: usize,
    finished: bool,
}

impl RequestDecoder {
    pub fn with_max_length(max_length: usize) -> Self {
        Self {
            max_length,
            next_index: 0,
            finished: false,
        }
    }
}

impl Decoder for RequestDecoder {
    type Item = Request;
    type Error = RequestError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        assert!(!self.finished, "RequestDecoder called in invalid state");

        // RFC 1436 (Gopher protocol) says a selector may have any character except:
        //  - TAB
        //  - CR-LF sequence (becaue it's terminated by that)
        //  - NUL
        // RFC 1738 (URLs) says a selector may have any character except:
        //  - TAB
        //  - LF
        //  - CR
        // This reader is going to forbid all of these.
        // Additionally we impose the requirement that the selector is UTF-8.

        let read_to = std::cmp::min(self.max_length + 2, buf.len());

        let offset = buf[self.next_index .. read_to]
            .windows(2)
            .enumerate()
            .filter_map(|(i, pair)| {
                let invalid = |c| match c {
                    b'\r' | b'\n' | b'\t' | b'\0' => true,
                    _ => false,
                };
                match pair {
                    [b'\r', b'\n'] => Some(Ok(i)),
                    [first, b'\r'] => if invalid(*first) { Some(Err(i)) } else { None },
                    [first, second] if invalid(*first) || invalid(*second) => Some(Err(i)),
                    _ => None,
                }
            })
            .next();

        match offset {
            Some(Ok(offset)) => {
                // Found a line.
                let newline_index = offset + self.next_index;
                let bytes = buf.split_to(newline_index + 2);
                let line = std::str::from_utf8(&bytes[..newline_index])
                    .map_err(RequestError::Utf8)?;
                self.finished = true;
                Ok(Some(Request { selector: line.to_owned() }))
            }
            Some(Err(offset)) => {
                // Invalid selector.
                let msg = format!("selector {:?} contains invalid characters at {}",
                    String::from_utf8_lossy(&buf), offset);
                Err(RequestError::InvalidSelector(msg))
            }
            None if buf.len() > self.max_length => {
                self.finished = true;
                Err(RequestError::TooLong)
            }
            None => {
                // Request the caller to read some more data into the buffer.
                Ok(None)
            }
        }
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.decode(buf)? {
            Some(request) => Ok(Some(request)),
            None => {
                // Request not terminated with CRLF.
                Err(RequestError::InvalidSelector("missing CR-LF".into()))
            }
        }
    }
}

pub struct RequestReader<R> {
    inner: tokio_util::codec::FramedRead<R, RequestDecoder>,
}

impl<R: tokio::io::AsyncRead + Unpin> RequestReader<R> {
    pub fn with_max_length(max_length: usize, async_read: R) -> Self {
        Self {
            inner: tokio_util::codec::FramedRead::new(
                       async_read,
                       RequestDecoder::with_max_length(max_length),
            ),
        }
    }

    pub async fn read_request(mut self) -> Result<Request, RequestError> {
        // This is a little weird. FramedRead is a stream of "frames", but Gopher protocol always
        // has only one request per connection, so we just take the first one, consuming Self in
        // the process.
        // Note that this means any garbage after the first CR-LF will be discarded and silently
        // ignored, because CR-LF is what separates frames.
        self.inner.next()
            .await
            .unwrap_or(Err(RequestError::InvalidSelector("missing CR-LF".into())))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn full_line() {
        let mut decoder = RequestDecoder::with_max_length(100);
        let mut buf = BytesMut::from("foo bar\r\nbaz");
        assert_eq!(decoder.decode(&mut buf).unwrap().unwrap().selector, "foo bar");
        assert_eq!(&buf, &b"baz"[..]);
        assert!(decoder.finished);
    }

    #[test]
    fn partial_line() {
        let mut decoder = RequestDecoder::with_max_length(100);
        let mut buf = BytesMut::from("foo");
        assert!(decoder.decode(&mut buf).unwrap().is_none());
        assert_eq!(buf, "foo");
        assert!(!decoder.finished);
    }

    #[test]
    fn too_long() {
        let mut decoder = RequestDecoder::with_max_length(3);
        let mut buf = BytesMut::from("foobar");
        match decoder.decode(&mut buf) {
            Err(RequestError::TooLong) => (),
            other => panic!("unexpected result {:?}", other),
        }
        assert!(decoder.finished);
    }

    #[test]
    fn empty() {
        let mut decoder = RequestDecoder::with_max_length(100);
        let mut buf = BytesMut::from("");
        assert!(decoder.decode(&mut buf).unwrap().is_none());
    }

    #[tokio::test]
    async fn empty_reader() {
        let input = "";
        let reader = RequestReader::with_max_length(100, Cursor::new(input));
        match reader.read_request().await {
            Err(RequestError::InvalidSelector) => (),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn bad_chars() {
        macro_rules! check {
            ($e:expr) => {
                let mut decoder = RequestDecoder::with_max_length(100);
                match decoder.decode(&mut BytesMut::from("abcd\ref")) {
                    Err(RequestError::InvalidSelector) => (),
                    other => panic!("unexpected result {:?}", other),
                }
            }
        }

        check!("abc\rdef");
        check!("abc\r");
        check!("abc\ndef\r\n");
        check!("abc\0def\r\n");
        check!("abc\tdef\r\n");
    }
}
