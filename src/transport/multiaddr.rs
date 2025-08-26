use libp2p::{
    multiaddr::{Multiaddr, Protocol},
    multihash, PeerId,
};

/// A parsed WebTransport multiaddress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebTransportMultiaddr {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) certhashes: Vec<multihash::Multihash<64>>,
    pub(crate) remote_peer_id: Option<PeerId>,
}

impl WebTransportMultiaddr {
    /// Parses a multiaddress into a `WebTransportMultiaddr` for listening.
    pub(crate) fn from_listen_multiaddr(addr: &Multiaddr) -> Option<Self> {
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
            Protocol::QuicV1 => {}
            _ => return None,
        }

        if !matches!(iter.next(), Some(Protocol::WebTransport)) {
            return None;
        }

        Some(Self {
            host,
            port,
            certhashes: vec![],
            remote_peer_id: None,
        })
    }

    /// Parses a multiaddress into a `WebTransportMultiaddr` for dialing.
    pub(crate) fn from_dial_multiaddr(addr: &Multiaddr) -> Option<Self> {
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
            Protocol::QuicV1 => {}
            _ => return None,
        }

        if !matches!(iter.next(), Some(Protocol::WebTransport)) {
            return None;
        }

        let mut certhashes = Vec::new();
        let mut remote_peer_id = None;

        for proto in iter {
            match proto {
                Protocol::Certhash(hash) => {
                    certhashes.push(hash);
                }
                Protocol::P2p(peer_id_multihash) => {
                    remote_peer_id = Some(peer_id_multihash);
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