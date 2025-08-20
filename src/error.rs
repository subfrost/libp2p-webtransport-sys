//! Error types for the WebTransport transport.

use thiserror::Error;

/// Error that can happen when dialing a peer.
#[derive(Debug, Error)]
pub enum Error {
    /// An error occurred during the transport.
    #[error("Transport error: {0}")]
    Transport(#[from] std::io::Error),
    /// The multiaddress is not a valid WebTransport multiaddress.
    #[error("Invalid multiaddress: {0}")]
    InvalidMultiaddr(multiaddr::Multiaddr),
    /// An error occurred during a wtransport connecting.
    #[error("WTransport connecting error: {0}")]
    WTransportConnecting(#[from] wtransport::error::ConnectingError),
    /// An error occurred on a wtransport connection.
    #[error("WTransport connection error: {0}")]
    WTransportConnection(#[from] wtransport::error::ConnectionError),
    /// The certificate hash is invalid.
    #[error("Invalid certificate hash")]
    InvalidCerthash,
    /// An error occurred during the Noise handshake.
    #[error("Noise handshake error: {0}")]
    Noise(#[from] libp2p_noise::Error),
    /// The remote peer ID is missing from the multiaddress.
    #[error("Remote peer ID is missing from the multiaddress")]
    MissingRemotePeerId,
    /// An error occurred during a TLS operation.
    #[error("TLS error: {0}")]
    Tls(#[from] rcgen::Error),
}