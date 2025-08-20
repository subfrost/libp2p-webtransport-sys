#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! A pure Rust implementation of the [WebTransport](https://w3c.github.io/webtransport/) protocol for libp2p.
//!
//! This crate provides a [Transport](libp2p_core::Transport) that can be used to establish
//! WebTransport connections with other libp2p peers.

pub mod error;
pub mod stream;
pub mod transport;

pub use transport::WebTransport;
