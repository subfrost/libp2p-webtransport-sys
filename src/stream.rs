//! A stream muxer for WebTransport connections.

use futures::{
    io::{AsyncRead, AsyncWrite},
    Future,
};
use libp2p::core::muxing::{StreamMuxer, StreamMuxerEvent};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use wtransport::{
    endpoint::endpoint_side::Client,
    stream::{RecvStream, SendStream},
    Connection,
};

/// A stream muxer for WebTransport connections.
pub struct Muxer {
    conn: Connection,
    endpoint: Option<wtransport::Endpoint<Client>>,
    inbound_fut: Option<Pin<Box<dyn Future<Output = Result<(SendStream, RecvStream), wtransport::error::ConnectionError>> + Send>>>,
    outbound_fut: Option<Pin<Box<dyn Future<Output = Result<(SendStream, RecvStream), io::Error>> + Send>>>,
}

impl Muxer {
    /// Creates a new `Muxer`.
    pub fn new(conn: Connection, endpoint: Option<wtransport::Endpoint<Client>>) -> Self {
        Self {
            conn,
            endpoint,
            inbound_fut: None,
            outbound_fut: None,
        }
    }
}

pub struct BiStream {
    send: Compat<SendStream>,
    recv: Compat<RecvStream>,
}

impl AsyncRead for BiStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for BiStream {
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
    type Substream = BiStream;
    type Error = io::Error;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let self_mut = self.get_mut();
        if self_mut.inbound_fut.is_none() {
            self_mut.inbound_fut = Some(Box::pin(self_mut.conn.accept_bi()));
        }

        let fut = self_mut.inbound_fut.as_mut().unwrap();
        match fut.as_mut().poll(cx) {
            Poll::Ready(Ok((send, recv))) => {
                self_mut.inbound_fut = None;
                Poll::Ready(Ok(BiStream {
                    send: send.compat_write(),
                    recv: recv.compat(),
                }))
            }
            Poll::Ready(Err(e)) => {
                self_mut.inbound_fut = None;
                Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let self_mut = self.get_mut();
        if self_mut.outbound_fut.is_none() {
            let conn = self_mut.conn.clone();
            self_mut.outbound_fut = Some(Box::pin(async move {
                let opening_bi = conn
                    .open_bi()
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                opening_bi
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            }));
        }

        let fut = self_mut.outbound_fut.as_mut().unwrap();
        match fut.as_mut().poll(cx) {
            Poll::Ready(Ok((send, recv))) => {
                self_mut.outbound_fut = None;
                Poll::Ready(Ok(BiStream {
                    send: send.compat_write(),
                    recv: recv.compat(),
                }))
            }
            Poll::Ready(Err(e)) => {
                self_mut.outbound_fut = None;
                Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.conn.close(0u32.into(), b"close");
        if let Some(endpoint) = &self.endpoint {
            endpoint.close(0u32.into(), b"close");
        }
        Poll::Ready(Ok(()))
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        Poll::Pending
    }
}