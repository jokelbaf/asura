use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use session::Client;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::Policy;

pub struct Witness<S> {
    inner: S,
    pending: Option<(Arc<Policy>, Client)>,
}

impl<S> Witness<S> {
    pub fn new(inner: S, policy: Arc<Policy>, client: Client) -> Self {
        Self {
            inner,
            pending: Some((policy, client)),
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Witness<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(context, buffer);

        if buffer.filled().len() > before
            && let Some((policy, client)) = self.pending.take()
        {
            policy.vouch(&client);
        }

        result
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Witness<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}
