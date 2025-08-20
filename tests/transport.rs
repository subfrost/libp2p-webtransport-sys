use libp2p_core::{
    identity,
    multiaddr::{Multiaddr, Protocol},
    transport::{Transport, TransportEvent},
};
use libp2p_webtransport_sys::WebTransport;

#[tokio::test]
async fn dial_and_listen() {
    let id_keys = identity::Keypair::generate_ed25519();
    let mut transport = WebTransport::new(id_keys);

    let mut addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1/webtransport".parse().unwrap();
    transport.listen_on(1.into(), addr.clone()).unwrap();

    let listener_addr = match transport.next().await.unwrap() {
        TransportEvent::NewAddress { listen_addr, .. } => listen_addr,
        _ => panic!("Expected NewAddress event"),
    };

    let mut dialer = WebTransport::new(identity::Keypair::generate_ed25519());
    let mut dial = dialer.dial(listener_addr).unwrap();

    let (res_dial, res_listen) = futures::future::join(dial, transport.next()).await;

    assert!(res_dial.is_ok());
    assert!(res_listen.is_ok());
}