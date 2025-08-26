// NOTE: This test needs to be run with the `timeout` command on Linux, e.g.
// `timeout 120 RUST_LOG=debug cargo test -- --nocapture --test-threads=1`
// NOTE: This test needs to be run with the `timeout` command on Linux, e.g.
// `timeout 120 RUST_LOG=debug cargo test -- --nocapture --test-threads=1`
use futures::{channel::mpsc, future::poll_fn, FutureExt, SinkExt, StreamExt};
use log;
use libp2p::{
    core::transport::{DialOpts, ListenerId, TransportEvent},
    core::Endpoint,
    identity,
    multiaddr::{Multiaddr, Protocol},
    Transport,
};
use libp2p_webtransport_sys::transport::WebTransport;
use std::pin::Pin;

#[tokio::test]
async fn close_listener() {
    let _ = env_logger::try_init();
    let id_keys = identity::Keypair::generate_ed25519();
    let mut transport = WebTransport::new(id_keys);

    assert!(
        poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
            .now_or_never()
            .is_none()
    );

    // Run test twice to check that there is no unexpected behaviour if `Transport.listener`
    // is temporarily empty.
    for _ in 0..2 {
        let id = ListenerId::next();
        transport
            .listen_on(id, "/ip4/127.0.0.1/udp/0/quic-v1/webtransport".parse().unwrap())
            .unwrap();

        match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
            TransportEvent::NewAddress {
                listener_id,
                listen_addr,
            } => {
                assert_eq!(listener_id, id);
                assert!(
                    matches!(listen_addr.iter().next(), Some(Protocol::Ip4(a)) if !a.is_unspecified())
                );
                assert!(
                    matches!(listen_addr.iter().nth(1), Some(Protocol::Udp(port)) if port != 0)
                );
                assert!(matches!(
                    listen_addr.iter().nth(2),
                    Some(Protocol::QuicV1)
                ));
                assert!(matches!(
                    listen_addr.iter().nth(3),
                    Some(Protocol::WebTransport)
                ));
            }
            e => panic!("Unexpected event: {e:?}"),
        }
        assert!(transport.remove_listener(id));
        match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
            TransportEvent::ListenerClosed {
                listener_id,
                reason: Ok(()),
            } => {
                assert_eq!(listener_id, id);
            }
            e => panic!("Unexpected event: {e:?}"),
        }
        // Poll once again so that the listener has the chance to return `Poll::Ready(None)` and
        // be removed from the list of listeners.
        assert!(
            poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
                .now_or_never()
                .is_none()
        );
    }
}

#[tokio::test]
async fn dial_and_listen() {
    let _ = env_logger::try_init();
    log::info!("starting dial_and_listen test");
    let id_keys = identity::Keypair::generate_ed25519();
    let mut listener = WebTransport::new(id_keys.clone());

    let addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1/webtransport".parse().unwrap();
    log::debug!("listening on {}", addr);
    listener.listen_on(ListenerId::next(), addr).unwrap();

    log::debug!("polling for NewAddress event");
    let listener_addr = match poll_fn(|cx| Pin::new(&mut listener).as_mut().poll(cx)).await {
        TransportEvent::NewAddress { listen_addr, .. } => listen_addr,
        e => panic!("Expected NewAddress event, got {:?}", e),
    };
    log::info!("listener address: {}", listener_addr);

    let (mut tx, mut rx) = mpsc::channel(1);

    tokio::spawn(async move {
        loop {
            match poll_fn(|cx| Pin::new(&mut listener).as_mut().poll(cx)).await {
                TransportEvent::Incoming { upgrade, .. } => {
                    let mut tx = tx.clone();
                    tokio::spawn(async move {
                        tx.send(upgrade.await).await.unwrap();
                    });
                }
                _ => {}
            }
        }
    });

    let mut dialer = WebTransport::new(identity::Keypair::generate_ed25519());
    let dial_addr = listener_addr.with(Protocol::P2p(id_keys.public().to_peer_id()));
    log::info!("dialing {}", dial_addr);
    let dial = dialer
        .dial(
            dial_addr,
            DialOpts {
                role: Endpoint::Dialer,
                port_use: libp2p::core::transport::PortUse::Reuse,
            },
        )
        .unwrap();

    let (res_dial, res_listen) = tokio::join!(dial, rx.next());
    log::info!("join completed");

    assert!(res_dial.is_ok(), "dial failed: {:?}", res_dial.err());
    let res_listen = res_listen.unwrap();
    assert!(res_listen.is_ok());

    log::info!("dial_and_listen test finished");
}