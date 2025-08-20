//! A stream muxer for WebTransport connections.

use futures::{AsyncRead, AsyncWrite};
use libp2p_core::muxing::{StreamMuxer, Substream, StreamMuxerEvent, StreamMuxerFuture};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use futures::future::BoxFuture;
use futures::FutureExt;
use wtransport::stream::OpeningBiStream;
use wtransport::{Connection, RecvStream, SendStream};

/// A stream muxer for WebTransport connections.
pub struct Muxer {
    conn: Connection,
    next_inbound_stream: Option<BoxFuture<'static, Result<Substream, io::Error>>>,
}

impl Muxer {
    /// Creates a new `Muxer`.
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            next_inbound_stream: None,
        }
    }
}

pub struct Substream {
    recv: RecvStream,
    send: SendStream,
}

impl AsyncRead for Substream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for Substream {
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

impl StreamMuxer for Muxer {
    type Substream = Substream;
    type Error = io::Error;
    type Event = StreamMuxerEvent;

    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        if self.next_inbound_stream.is_none() {
            let conn = self.conn.clone();
            self.next_inbound_stream = Some(
                async move {
                    let (send, recv) = conn
                        .accept_bi()
                        .await
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    Ok(Substream { recv, send })
                }
                .boxed(),
            );
        }

        if let Some(fut) = self.next_inbound_stream.as_mut() {
            let res = futures::ready!(fut.poll_unpin(cx));
            self.next_inbound_stream = None;
            return Poll::Ready(res);
        }

        Poll::Pending
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let conn = self.conn.clone();
        let fut = async move {
            let (send, recv) = conn
                .open_bi()
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            Ok(Substream { recv, send })
        };

        // We need to block on this future, which is not ideal in a poll function.
        // This is a limitation of the current wtransport API.
        // A better approach would be to have a poll-based API for opening streams.
        // For now, we'll use a temporary runtime to block on the future.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        Poll::Ready(runtime.block_on(fut))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.conn.close(0u32.into(), b"close");
        Poll::Ready(Ok(()))
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Event, Self::Error>> {
        Poll::Pending
    }
}