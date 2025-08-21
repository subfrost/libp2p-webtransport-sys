use futures_util::stream::StreamExt;
use libp2p::{
    core::transport::{ListenerId, TransportEvent},
    identity,
    multiaddr::{Multiaddr, Protocol},
    Transport,
};
use libp2p_webtransport_sys::transport as WebTransport;

#[tokio::test]
async fn dial_and_listen() {
    let _ = env_logger::try_init();
    let id_keys = identity::Keypair::generate_ed25519();
    let mut transport = WebTransport::new(id_keys);

    let addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1/webtransport"
        .parse()
        .unwrap();
    transport.listen_on(addr).unwrap();

    let listener_addr = match transport.select_next_some().await {
        TransportEvent::NewAddress { listen_addr, .. } => listen_addr,
        _ => panic!("Expected NewAddress event"),
    };

    let mut dialer = WebTransport::new(identity::Keypair::generate_ed25519());
    let dial = dialer.dial(listener_addr).unwrap();

    let (res_dial, res_listen) = futures::future::join(dial, transport.select_next_some()).await;

    assert!(res_dial.is_ok());
    assert!(res_listen.is_ok());
}