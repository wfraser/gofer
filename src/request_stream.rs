use crate::request::{Request, RequestError, RequestReader};
use futures::future::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::io;
use tokio::net::{TcpListener, ToSocketAddrs};
use tokio::net::tcp::OwnedWriteHalf;

pub struct RequestStream {
    listener: TcpListener,

    pending: FuturesUnordered<ReqWritePair>,
}

impl RequestStream {
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr).await?,
            pending: FuturesUnordered::new(),
        })
    }

    pub async fn next_request(&mut self) -> (Result<Request, RequestError>, OwnedWriteHalf) {
        loop {
            if self.pending.len() > 1 {
                eprintln!("{} pending requests", self.pending.len());
            }
            tokio::select! {
                Some((req_result, tx)) = self.pending.next(), if !self.pending.is_empty() => {

                    return (req_result, tx);
                }
                accept_res = self.listener.accept(),
                    if self.pending.len() < crate::MAX_QUEUED_REQUESTS =>
                {
                    match accept_res {
                        Ok((conn, remote_addr)) => {
                            eprintln!("got connection from {:?}", remote_addr);
                            let (rx, tx) = conn.into_split();
                            self.pending.push(Box::pin(
                                RequestReader::with_max_length(1024, rx)
                                    .read_request()
                                    .map(move |req_result| (req_result, tx))));
                        }
                        Err(e) => {
                            eprintln!("error accepting connection: {}", e);
                        }
                    }
                }
            };
        }
    }
}

// The future result of reading the request, and the associated write half of the connection.
type ReqWritePair = Pin<Box<dyn Future<Output=(Result<Request, RequestError>, OwnedWriteHalf)>>>;
