use futures::{AsyncRead, AsyncWrite};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio_util::compat::Compat;
use wtransport::{RecvStream, SendStream};

pub struct NoiseStream {
    pub(crate) recv: Compat<RecvStream>,
    pub(crate) send: Compat<SendStream>,
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