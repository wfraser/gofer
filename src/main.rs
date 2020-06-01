mod request;
mod request_stream;
mod types;

use crate::request::Request;
use crate::request_stream::RequestStream;
use std::io;
use tokio::io::AsyncWriteExt;

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut incoming = RequestStream::bind("0.0.0.0:7070").await?;

    loop {
        match incoming.next_request().await {
            (Ok(Request { selector }), mut tx) => {
                eprintln!("selector: {}", selector);        
                tx.write_all(b"iok!\r\ni").await?;
                tx.write_all(selector.as_bytes()).await?;
                tx.write_all(b"\r\n").await?;
            }
            (Err(e), mut tx) => {
                eprintln!("error: {:?}", e);
                tx.write_all(b"3Request Error\r\n3").await?;
                tx.write_all(format!("{:?}", e).as_bytes()).await?;
            }
        }
    }
}
