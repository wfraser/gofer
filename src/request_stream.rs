use crate::request::{Request, RequestError, RequestReader};
use std::io;
use tokio::stream::StreamExt;
use tokio::net::{TcpListener, ToSocketAddrs};
use tokio::net::tcp::OwnedWriteHalf;

pub struct RequestStream {
    listener: TcpListener,
}

impl RequestStream {
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr).await?,
        })
    }

    pub async fn next_request(&mut self) -> (Result<Request, RequestError>, OwnedWriteHalf) {
        let conn = loop {
            let result = self.listener.incoming()
                .next()
                .await
                .expect("unexpected end of connection stream from TcpListener");
            match result {
                Ok(conn) => {
                    eprintln!("got connection: {:?}", conn);
                    break conn
                }
                Err(e) => {
                    eprintln!("error accepting connection: {}", e);
                }
            }
        };

        let (rx, tx) = conn.into_split();

        let req_result = RequestReader::with_max_length(1024, rx)
            .read_request()
            .await;

        (req_result, tx)
    }
}
