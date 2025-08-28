// NOTE: This test needs to be run with the `timeout` command on Linux, e.g.
// `timeout 120 RUST_LOG=debug cargo test -- --nocapture --test-threads=1`
// NOTE: This test needs to be run with the `timeout` command on Linux, e.g.
// `timeout 120 RUST_LOG=debug cargo test -- --nocapture --test-threads=1`
use futures::{channel::mpsc, future::poll_fn, FutureExt, SinkExt, StreamExt};
use log;
use libp2p::{
    core::transport::{DialOpts, ListenerId, TransportEvent},
    identity,
    multiaddr::{Multiaddr, Protocol},
    Transport,
};
use libp2p_webtransport_sys::transport::{Config, WebTransport};
use multihash::Multihash;
use std::{pin::Pin, sync::Once};

static INIT: Once = Once::new();

fn init() {
    INIT.call_once(|| {
        let _ = env_logger::try_init();
        rustls::crypto::ring::default_provider()
            .install_default()
            .unwrap();
    });
}

#[tokio::test]
async fn close_listener() {
    init();
    let id_keys = identity::Keypair::generate_ed25519();
    let mut transport = WebTransport::new(id_keys, Config::SelfSigned);

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
    init();
    log::info!("starting dial_and_listen test");
    let id_keys = identity::Keypair::generate_ed25519();
    let mut listener = WebTransport::new(id_keys.clone(), Config::SelfSigned);

    let addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1/webtransport".parse().unwrap();
    log::debug!("listening on {}", addr);
    listener.listen_on(ListenerId::next(), addr).unwrap();

    log::debug!("polling for NewAddress event");
    let listener_addr = match poll_fn(|cx| Pin::new(&mut listener).as_mut().poll(cx)).await {
        TransportEvent::NewAddress { listen_addr, .. } => listen_addr,
        e => panic!("Expected NewAddress event, got {:?}", e),
    };
    log::info!("listener address: {}", listener_addr);

    let (tx, mut rx) = mpsc::channel(1);

    tokio::spawn(async move {
        loop {
            match poll_fn(|cx| Pin::new(&mut listener).as_mut().poll(cx)).await {
                TransportEvent::Incoming { upgrade, .. } => {
                    let mut tx = tx.clone();
                    tokio::spawn(async move {
                        log::info!("upgrading incoming connection");
                        let result = upgrade.await;
                        log::info!("incoming upgrade finished: {:?}", result);
                        tx.send(result).await.unwrap();
                    });
                }
                _ => {}
            }
        }
    });

    let mut dialer = WebTransport::new(identity::Keypair::generate_ed25519(), Config::SelfSigned);
    let dial_addr = listener_addr.with(Protocol::P2p(id_keys.public().to_peer_id()));
    log::info!("dialing {}", dial_addr);
    let dial = dialer
        .dial(
            dial_addr,
            DialOpts {
                role: libp2p::core::Endpoint::Dialer,
                port_use: libp2p::core::transport::PortUse::Reuse,
            },
        )
        .unwrap();

    let dial_task = tokio::spawn(dial);

    let (res_dial, res_listen) = tokio::join!(dial_task, rx.next());
    log::info!("join completed");
    log::info!("dial task result: {:?}", res_dial);
    log::info!("listen task result: {:?}", res_listen);

    assert!(res_dial.is_ok(), "dial task failed");
    let res_dial = res_dial.unwrap();
    assert!(res_dial.is_ok(), "dial failed: {:?}", res_dial.err());
    let res_listen = res_listen.unwrap();
    assert!(res_listen.is_ok());

    log::info!("dial_and_listen test finished");
}

#[tokio::test]
async fn dial_and_listen_with_certificate() {
    init();
    log::info!("starting dial_and_listen_with_certificate test");

    // 1. Generate and save a certificate and private key
    let mut cert_params = rcgen::CertificateParams::new(vec!["127.0.0.1".into()]);
    cert_params.alg = &rcgen::PKCS_ECDSA_P256_SHA256;
    cert_params.not_before = time::OffsetDateTime::now_utc();
    cert_params.not_after =
        time::OffsetDateTime::now_utc() + std::time::Duration::from_secs(60 * 60 * 24 * 7);
    let cert = rcgen::Certificate::from_params(cert_params).unwrap();
    let cert_pem = cert.serialize_pem().unwrap();
    let key_pem = cert.serialize_private_key_pem();
    let cert_path = std::env::temp_dir().join("cert.pem");
    let key_path = std::env::temp_dir().join("key.pem");
    std::fs::write(&cert_path, cert_pem).unwrap();
    std::fs::write(&key_path, key_pem).unwrap();

    let id_keys = identity::Keypair::generate_ed25519();
    let config = Config::Certificate {
        certificate: cert_path.clone(),
        private_key: key_path.clone(),
    };
    let mut listener = WebTransport::new(id_keys.clone(), config);

    let addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1/webtransport".parse().unwrap();
    log::debug!("listening on {}", addr);
    listener.listen_on(ListenerId::next(), addr).unwrap();

    log::debug!("polling for NewAddress event");
    let listener_addr = match poll_fn(|cx| Pin::new(&mut listener).as_mut().poll(cx)).await {
        TransportEvent::NewAddress { listen_addr, .. } => listen_addr,
        e => panic!("Expected NewAddress event, got {:?}", e),
    };
    log::info!("listener address: {}", listener_addr);

    let (tx, mut rx) = mpsc::channel(1);

    tokio::spawn(async move {
        loop {
            match poll_fn(|cx| Pin::new(&mut listener).as_mut().poll(cx)).await {
                TransportEvent::Incoming { upgrade, .. } => {
                    let mut tx = tx.clone();
                    tokio::spawn(async move {
                        log::info!("upgrading incoming connection");
                        let result = upgrade.await;
                        log::info!("incoming upgrade finished: {:?}", result);
                        tx.send(result).await.unwrap();
                    });
                }
                _ => {}
            }
        }
    });

    let dial_addr = listener_addr.with(Protocol::P2p(id_keys.public().to_peer_id()));
    let mut dialer = WebTransport::new(identity::Keypair::generate_ed25519(), Config::SelfSigned);
    let dial = dialer
        .dial(
            dial_addr,
            DialOpts {
                role: libp2p::core::Endpoint::Dialer,
                port_use: libp2p::core::transport::PortUse::Reuse,
            },
        )
        .unwrap();

    let dial_task = tokio::spawn(dial);

    let (res_dial, res_listen) = tokio::join!(dial_task, rx.next());
    log::info!("join completed");
    log::info!("dial task result: {:?}", res_dial);
    log::info!("listen task result: {:?}", res_listen);

    assert!(res_dial.is_ok(), "dial task failed");
    let res_dial = res_dial.unwrap();
    assert!(res_dial.is_ok(), "dial failed: {:?}", res_dial);
    let res_listen = res_listen.unwrap();
    assert!(res_listen.is_ok());

    // Clean up the temporary files
    std::fs::remove_file(&cert_path).unwrap();
    std::fs::remove_file(&key_path).unwrap();

    log::info!("dial_and_listen_with_certificate test finished");
}

#[tokio::test]
async fn dial_and_listen_with_certificate_from_memory() {
    init();
    log::info!("starting dial_and_listen_with_certificate_from_memory test");

    // 1. Generate a certificate and private key
    let mut cert_params = rcgen::CertificateParams::new(vec!["127.0.0.1".into()]);
    cert_params.alg = &rcgen::PKCS_ECDSA_P256_SHA256;
    cert_params.not_before = time::OffsetDateTime::now_utc();
    cert_params.not_after =
        time::OffsetDateTime::now_utc() + std::time::Duration::from_secs(60 * 60 * 24 * 7);
    let cert = rcgen::Certificate::from_params(cert_params).unwrap();
    let cert_pem = cert.serialize_pem().unwrap().as_bytes().to_vec();
    let key_pem = cert.serialize_private_key_pem().as_bytes().to_vec();

    let id_keys = identity::Keypair::generate_ed25519();
    let config = Config::CertificateFromMemory {
        certificate: cert_pem.clone(),
        private_key: key_pem.clone(),
    };
    let mut listener = WebTransport::new(id_keys.clone(), config);

    let addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1/webtransport".parse().unwrap();
    log::debug!("listening on {}", addr);
    listener.listen_on(ListenerId::next(), addr).unwrap();

    log::debug!("polling for NewAddress event");
    let listener_addr = match poll_fn(|cx| Pin::new(&mut listener).as_mut().poll(cx)).await {
        TransportEvent::NewAddress { listen_addr, .. } => listen_addr,
        e => panic!("Expected NewAddress event, got {:?}", e),
    };
    log::info!("listener address: {}", listener_addr);

    let (tx, mut rx) = mpsc::channel(1);

    tokio::spawn(async move {
        loop {
            match poll_fn(|cx| Pin::new(&mut listener).as_mut().poll(cx)).await {
                TransportEvent::Incoming { upgrade, .. } => {
                    let mut tx = tx.clone();
                    tokio::spawn(async move {
                        log::info!("upgrading incoming connection");
                        let result = upgrade.await;
                        log::info!("incoming upgrade finished: {:?}", result);
                        tx.send(result).await.unwrap();
                    });
                }
                _ => {}
            }
        }
    });

    let dial_addr = listener_addr.with(Protocol::P2p(id_keys.public().to_peer_id()));
    let mut dialer = WebTransport::new(identity::Keypair::generate_ed25519(), Config::SelfSigned);
    let dial = dialer
        .dial(
            dial_addr,
            DialOpts {
                role: libp2p::core::Endpoint::Dialer,
                port_use: libp2p::core::transport::PortUse::Reuse,
            },
        )
        .unwrap();

    let dial_task = tokio::spawn(dial);

    let (res_dial, res_listen) = tokio::join!(dial_task, rx.next());
    log::info!("join completed");
    log::info!("dial task result: {:?}", res_dial);
    log::info!("listen task result: {:?}", res_listen);

    assert!(res_dial.is_ok(), "dial task failed");
    let res_dial = res_dial.unwrap();
    assert!(res_dial.is_ok(), "dial failed: {:?}", res_dial);
    let res_listen = res_listen.unwrap();
    assert!(res_listen.is_ok());

    log::info!("dial_and_listen_with_certificate_from_memory test finished");
}

#[tokio::test]
async fn dial_and_listen_with_ec_key() {
    init();

    // 1. Generate EC key and certificate
    let cert = generate_ec_cert_and_key();

    // 2. Set up listener transport with the new cert/key from memory
    let listener_keypair = identity::Keypair::generate_ed25519();
    let listener_peer_id = listener_keypair.public().to_peer_id();
    let mut listener_transport = WebTransport::new(
        listener_keypair,
        Config::CertificateFromMemory {
            certificate: cert.serialize_pem().unwrap().as_bytes().to_vec(),
            private_key: cert.serialize_private_key_pem().as_bytes().to_vec(),
        },
    );

    // 3. Start listening
    let addr = "/ip4/127.0.0.1/udp/0/quic-v1/webtransport"
        .parse()
        .unwrap();
    listener_transport.listen_on(ListenerId::next(), addr).unwrap();
    let listen_addr = wait_for_listen_addr(&mut listener_transport).await;
    let certhash = get_certhash(&listen_addr);

    // 4. Set up dialer transport
    let dialer_keypair = identity::Keypair::generate_ed25519();
    let dialer_peer_id = dialer_keypair.public().to_peer_id();
    let mut dialer_transport = WebTransport::new(dialer_keypair, Config::SelfSigned);

    // 5. Spawn listener task
    tokio::spawn(async move {
        loop {
            match poll_fn(|cx| Pin::new(&mut listener_transport).as_mut().poll(cx)).await {
                TransportEvent::Incoming { upgrade, .. } => {
                    let (peer, _muxer) = upgrade.await.unwrap();
                    assert_eq!(peer, dialer_peer_id);
                    break;
                }
                _ => {}
            }
        }
        
        
    });

    // 6. Dial the listener
    let dial_addr = listen_addr
        .with(Protocol::P2p(listener_peer_id))
        .with(Protocol::Certhash(certhash));
    let (_peer, _muxer) = dialer_transport
        .dial(
            dial_addr,
            DialOpts {
                role: libp2p::core::Endpoint::Dialer,
                port_use: libp2p::core::transport::PortUse::Reuse,
            },
        )
        .unwrap()
        .await
        .unwrap();
}

// Helper function to generate EC certificate and key
fn generate_ec_cert_and_key() -> rcgen::Certificate {
    let mut params = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()]);
    params.alg = &rcgen::PKCS_ECDSA_P256_SHA256; // Use an EC algorithm
    rcgen::Certificate::from_params(params).unwrap()
}

async fn wait_for_listen_addr(transport: &mut WebTransport) -> Multiaddr {
    loop {
        match poll_fn(|cx| Pin::new(&mut *transport).as_mut().poll(cx)).await {
            TransportEvent::NewAddress { listen_addr, .. } => return listen_addr,
            _ => {}
        }
    }
}

fn get_certhash(addr: &Multiaddr) -> Multihash<64> {
    let mut certhash = None;
    for p in addr.iter() {
        if let Protocol::Certhash(hash) = p {
            certhash = Some(hash);
        }
    }
    certhash.unwrap()
}