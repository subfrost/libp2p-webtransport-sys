# libp2p-webtransport-sys

This crate provides a pure Rust implementation of the [WebTransport](https://w3c.github.io/webtransport/) protocol for libp2p.

## Overview

This transport allows libp2p nodes to connect to each other using the WebTransport protocol, which is a modern, secure, and multiplexed transport based on HTTP/3 and QUIC.

## Features

-   Pure Rust implementation.
-   Integration with the `libp2p-core` `Transport` trait.
-   Client and server support.
-   Noise-based handshake for security.

## Usage

To use this crate, add it to your `Cargo.toml`:

```toml
[dependencies]
libp2p-webtransport-sys = { git = "https://github.com/libp2p/rust-libp2p" }
```

Then, you can create a new `WebTransport` transport and add it to your `Swarm`:

```rust
use libp2p_webtransport_sys::WebTransport;
use libp2p_core::transport::Transport;
use libp2p_core::identity;

let local_key = identity::Keypair::generate_ed25519();
let transport = WebTransport::new(local_key);