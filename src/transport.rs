//! The WebTransport transport.

use crate::error::Error;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, Transport, TransportError, TransportEvent},
    PeerId,
};
use std::{
    collections::HashMap,
    pin::Pin,
    task::{Context, Poll},
};
use futures::{future::Future, stream::Stream, AsyncRead, AsyncWrite, StreamExt};
use libp2p_core::identity;
use libp2p_core::muxing::StreamMuxer;
use std::io;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use wtransport::{RecvStream, SendStream};

/// The WebTransport transport.
pub struct WebTransport {
    keypair: identity::Keypair,
    listeners: HashMap<ListenerId, (mpsc::Receiver<TransportEvent<ListenerUpgrade, Dial, Error>>, oneshot::Sender<()>)>,
}

type Dial = Pin<Box<dyn Future<Output = Result<Output, Error>> + Send>>;
type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Output, Error>> + Send>>;
type Output = (PeerId, crate::stream::Muxer);

impl WebTransport {
    /// Creates a new WebTransport transport.
    pub fn new(keypair: identity::Keypair) -> Self {
        Self {
            keypair,
            listeners: HashMap::new(),
        }
    }
}

struct NoiseStream {
    recv: Compat<RecvStream>,
    send: Compat<SendStream>,
}

impl AsyncRead for NoiseStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for NoiseStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.send).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.send).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.send).poll_close(cx)
    }
}

/// A parsed WebTransport multiaddress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebTransportMultiaddr {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) certhashes: Vec<multihash::Multihash>,
    pub(crate) remote_peer_id: Option<PeerId>,
}

impl WebTransportMultiaddr {
    /// Parses a multiaddress into a `WebTransportMultiaddr`.
    pub(crate) fn from_multiaddr(addr: &Multiaddr) -> Option<Self> {
        let mut iter = addr.iter();

        let (host, port) = match (iter.next()?, iter.next()?) {
            (Protocol::Ip4(ip), Protocol::Udp(p)) => (ip.to_string(), p),
            (Protocol::Ip6(ip), Protocol::Udp(p)) => (format!("[{}]", ip), p),
            (Protocol::Dns(dns), Protocol::Udp(p))
            | (Protocol::Dns4(dns), Protocol::Udp(p))
            | (Protocol::Dns6(dns), Protocol::Udp(p)) => (dns.to_string(), p),
            _ => return None,
        };

        match iter.next()? {
            Protocol::QuicV1 | Protocol::Quic => {} // Allow both for now
            _ => return None,
        }

        if !matches!(iter.next(), Some(Protocol::Webtransport)) {
            return None;
        }

        let mut certhashes = Vec::new();
        let mut remote_peer_id = None;

        for proto in iter {
            match proto {
                Protocol::Certhash(hash) => {
                    certhashes.push(hash.into());
                }
                Protocol::P2p(peer_id_multihash) => {
                    remote_peer_id = PeerId::from_multihash(peer_id_multihash).ok();
                }
                _ => {}
            }
        }

        if certhashes.is_empty() {
            return None;
        }

        Some(Self {
            host,
            port,
            certhashes,
            remote_peer_id,
        })
    }
}

impl Transport for WebTransport {
    type Output = Output;
    type Error = Error;
    type ListenerUpgrade = ListenerUpgrade;
    type Dial = Dial;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        let s_addr = WebTransportMultiaddr::from_multiaddr(&addr)
            .ok_or(TransportError::Other(Error::InvalidMultiaddr(addr.clone())))?;

        let (tx, rx) = mpsc::channel(1);
        let (stop_tx, stop_rx) = oneshot::channel();

        let keypair = self.keypair.clone();

        tokio::spawn(async move {
            let cert = rcgen::generate_simple_self_signed(vec![s_addr.host.clone()]).unwrap();
            let identity = wtransport::Identity::new(
                wtransport::tls::CertificateChain::new(vec![wtransport::tls::Certificate::from_der(
                    cert.serialize_der().unwrap(),
                )]),
                wtransport::tls::PrivateKey::from_der_pkcs8(cert.serialize_private_key_der()),
            );

            let server_config = wtransport::ServerConfig::builder()
                .with_bind_default(s_addr.port)
                .with_identity(identity)
                .build();

            let endpoint = wtransport::Endpoint::server(server_config).unwrap();

            let mut stop_rx = stop_rx;

            loop {
                tokio::select! {
                    _ = &mut stop_rx => {
                        break;
                    }
                    incoming_session = endpoint.accept() => {
                        if let Some(session_request) = incoming_session.await {
                            let conn = session_request.await.unwrap();
                            let (send, recv) = conn.open_bi().await.unwrap().await.unwrap();
                            let noise_stream = NoiseStream {
                                recv: recv.compat(),
                                send: send.compat_write(),
                            };

                            let noise_config = libp2p_noise::Config::new(&keypair).unwrap();
                            let (peer_id, _noise_output) = libp2p_noise::secure_inbound(noise_config, noise_stream).await.unwrap();

                            let muxer = crate::stream::Muxer::new(conn);
                            let event = TransportEvent::Incoming {
                                listener_id: id,
                                upgrade: Box::pin(async move { Ok((peer_id, muxer)) }),
                                local_addr: addr.clone(),
                                remote_addr: addr.clone(), // TODO: get from conn
                            };
                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.listeners.insert(id, (rx, stop_tx));

        Ok(())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.listeners.remove(&id).is_some()
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let s_addr = WebTransportMultiaddr::from_multiaddr(&addr)
            .ok_or(TransportError::Other(Error::InvalidMultiaddr(addr)))?;

        let keypair = self.keypair.clone();

        Ok(Box::pin(async move {
            let hashes = s_addr
                .certhashes
                .into_iter()
                .map(|mh| {
                    mh.digest()
                        .try_into()
                        .map(wtransport::tls::Sha256Digest::new)
                        .map_err(|_| Error::InvalidCerthash)
                })
                .collect::<Result<Vec<_>, _>>()?;

            let client_config = wtransport::ClientConfig::builder()
                .with_bind_default()
                .with_server_certificate_hashes(hashes)
                .build();

            let endpoint = wtransport::Endpoint::client(client_config)?;
            let url = format!(
                "https://{}:{}/.well-known/libp2p-webtransport",
                s_addr.host, s_addr.port
            );
            let conn = endpoint.connect(url).await?;

            let (send, recv) = conn.open_bi().await?.await?;
            let noise_stream = NoiseStream {
                recv: recv.compat(),
                send: send.compat_write(),
            };

            let remote_peer_id =
                s_addr.remote_peer_id.ok_or(Error::MissingRemotePeerId)?;
            let noise_config = libp2p_noise::Config::new(&keypair)?;
            let (peer_id, _noise_output) =
                libp2p_noise::secure_outbound(noise_config, noise_stream, remote_peer_id).await?;

            Ok((peer_id, crate::stream::Muxer::new(conn)))
        }))
    }

    fn dial_as_peer(
        &mut self,
        addr: Multiaddr,
        _peer_id: PeerId,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        // TODO: use peer_id for authentication
        self.dial(addr)
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Dial, Self::Error>> {
        for (id, (rx, _)) in self.listeners.iter_mut() {
            match rx.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => return Poll::Ready(event),
                Poll::Ready(None) => {
                    // TODO: Handle listener closed
                }
                Poll::Pending => {}
            }
        }

        Poll::Pending
    }

    fn address_translation(&self, _listen: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}