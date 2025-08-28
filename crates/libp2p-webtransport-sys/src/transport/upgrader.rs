use futures::{AsyncRead, AsyncWrite};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio_util::compat::Compat;
use wtransport::{RecvStream, SendStream};

/// A stream that has been upgraded to use the Noise protocol.
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
        let poll = Pin::new(&mut self.recv).poll_read(cx, buf);
        log::trace!("[NoiseStream::poll_read] poll: {:?}", poll);
        poll
    }
}

impl AsyncWrite for NoiseStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let poll = Pin::new(&mut self.send).poll_write(cx, buf);
        log::trace!("[NoiseStream::poll_write] poll: {:?}", poll);
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let poll = Pin::new(&mut self.send).poll_flush(cx);
        log::trace!("[NoiseStream::poll_flush] poll: {:?}", poll);
        poll
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let poll = Pin::new(&mut self.send).poll_close(cx);
        log::trace!("[NoiseStream::poll_close] poll: {:?}", poll);
        poll
    }
}