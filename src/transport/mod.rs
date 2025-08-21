//! The WebTransport transport.

pub mod multiaddr;
pub mod upgrader;

use crate::error::Error;
use async_trait::async_trait;
use futures::{future::Future};
use libp2p::{
    core::{
        muxing::StreamMuxerBox,
        transport::{DialOpts, ListenerId, TransportEvent},
        upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade},
    },
    identity, noise, PeerId,
};
pub use multiaddr::WebTransportMultiaddr;
use std::{
    collections::HashMap,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// The WebTransport transport.
pub struct WebTransport {
    keypair: identity::Keypair,
    listeners:
        HashMap<ListenerId, (mpsc::Receiver<TransportEvent<ListenerUpgrade, Error>>, oneshot::Sender<()>)>,
}

type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Output, Error>> + Send>>;
type Output = (PeerId, StreamMuxerBox);

impl WebTransport {
    /// Creates a new WebTransport transport.
    pub fn new(keypair: identity::Keypair) -> Self {
        Self {
            keypair,
            listeners: HashMap::new(),
        }
    }
}

#[async_trait]
impl libp2p::Transport for WebTransport {
    type Output = Output;
    type Error = Error;
    type ListenerUpgrade = ListenerUpgrade;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: libp2p::Multiaddr,
    ) -> Result<(), libp2p::core::transport::TransportError<Self::Error>> {
        let s_addr = WebTransportMultiaddr::from_multiaddr(&addr)
            .ok_or(libp2p::core::transport::TransportError::Other(Error::InvalidMultiaddr(addr.clone())))?;

        let (tx, rx) = mpsc::channel(1);
        let (stop_tx, stop_rx) = oneshot::channel();

        let keypair = self.keypair.clone();

        tokio::spawn(async move {
            let cert = rcgen::generate_simple_self_signed(vec![s_addr.host.clone()]).unwrap();
            let identity = wtransport::Identity::new(
                wtransport::tls::CertificateChain::new(vec![wtransport::tls::Certificate::from_der(
                    cert.serialize_der().unwrap().into(),
                )
                .unwrap()]),
                wtransport::tls::PrivateKey::from_der_pkcs8(
                    cert.serialize_private_key_der().into(),
                ),
            );

            let server_config = wtransport::ServerConfig::builder()
                .with_bind_default(s_addr.port)
                .with_identity(identity)
                .build();

            let endpoint = wtransport::Endpoint::server(server_config).unwrap();
            loop {
                let incoming_session = endpoint.accept().await;
                let keypair = keypair.clone();
                let addr = addr.clone();
                let upgrade: ListenerUpgrade = Box::pin(async move {
                    let session_request = incoming_session.await?;
                    let conn = session_request.accept().await?;
                    let (send, recv) = conn.open_bi().await?.await?;
                    let noise_stream = upgrader::NoiseStream {
                        recv: recv.compat(),
                        send: send.compat_write(),
                    };

                    let noise_config = noise::Config::new(&keypair)?;
                    let (peer_id, _noise_output) =
                        noise_config.upgrade_inbound(noise_stream, "").await?;

                    let muxer = crate::stream::Muxer::new(conn, None);
                    Ok((peer_id, StreamMuxerBox::new(muxer)))
                });

                let event = TransportEvent::Incoming {
                    listener_id: id,
                    upgrade,
                    local_addr: addr.clone(),
                    send_back_addr: addr.clone(), // TODO: get from conn
                };
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        self.listeners.insert(id, (rx, stop_tx));

        Ok(())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.listeners.remove(&id).is_some()
    }

    fn dial(
        &mut self,
        addr: libp2p::Multiaddr,
        _opts: DialOpts,
    ) -> Result<Self::Dial, libp2p::core::transport::TransportError<Self::Error>> {
        let s_addr = WebTransportMultiaddr::from_multiaddr(&addr)
            .ok_or(libp2p::core::transport::TransportError::Other(Error::InvalidMultiaddr(addr)))?;

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

            let endpoint = wtransport::Endpoint::client(client_config).unwrap();
            let url = format!(
                "https://{}:{}/.well-known/libp2p-webtransport",
                s_addr.host, s_addr.port
            );
            let conn = endpoint.connect(url).await?;

            let (send, recv) = conn.open_bi().await?.await?;
            let noise_stream = upgrader::NoiseStream {
                recv: recv.compat(),
                send: send.compat_write(),
            };

            let remote_peer_id =
                s_addr.remote_peer_id.ok_or(Error::MissingRemotePeerId)?;
            let noise_config = noise::Config::new(&keypair)?;
            let remote_peer_id_str = remote_peer_id.to_string();
            let (peer_id, _noise_output) = noise_config
                .upgrade_outbound(noise_stream, Box::leak(remote_peer_id_str.into_boxed_str()),)
                .await?;

            Ok((
                peer_id,
                StreamMuxerBox::new(crate::stream::Muxer::new(conn, Some(endpoint))),
            ))
        }))
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        for (_id, (rx, _)) in self.listeners.iter_mut() {
            if let Poll::Ready(Some(event)) = rx.poll_recv(cx) {
                return Poll::Ready(event);
            }
        }

        Poll::Pending
    }
}
