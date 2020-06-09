# GoFer

Gopher server experiment in Rust.

Pretty basic, just playing around with Tokio a bit.

The easiest way to browse Gopher these days is with Lynx.

Start the server using `cargo run config.toml`, which starts a server listening on port 7070, then
run `lynx gopher://127.0.0.1:7070` and bask in the amazing plain-text glory of what the pre-web
internet was like.
