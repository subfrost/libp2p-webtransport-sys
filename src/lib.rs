#![warn(missing_docs)]

//! A pure Rust implementation of the WebTransport protocol for libp2p.
//!
//! This crate provides a transport that can be used to establish
//! WebTransport connections with other libp2p peers.
//!
//! # Usage
//!
//! ```
//! use libp2p_webtransport_sys::transport::WebTransport;
//! use libp2p::identity;
//!
//! #[tokio::main]
//! async fn main() {
//!     let local_key = identity::Keypair::generate_ed25519();
//!     let mut transport = WebTransport::new(local_key);
//! }
//! ```

pub mod error;
pub mod stream;
pub mod transport;

pub use transport::WebTransport;
