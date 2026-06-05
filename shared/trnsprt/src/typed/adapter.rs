//! `Adapter` trait + concrete implementations (TCP, async stdio, in-proc).
//!
//! An adapter delivers raw bytes. It splits into an `AsyncRead` half and
//! an `AsyncWrite` half so a `Channel` can frame both directions
//! independently.

use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::mpsc;

use super::error::AdapterError;

pub type DynRead = Box<dyn AsyncRead + Unpin + Send>;
pub type DynWrite = Box<dyn AsyncWrite + Unpin + Send>;

pub trait Adapter: Send + 'static {
    fn split(self: Box<Self>) -> (DynRead, DynWrite);
}

// ---------------------------------------------------------------- TcpAdapter

pub struct TcpAdapter {
    stream: TcpStream,
}

impl TcpAdapter {
    pub fn new(stream: TcpStream) -> Self {
        Self { stream }
    }

    pub async fn connect(addr: &str) -> Result<Self, AdapterError> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self { stream })
    }
}

impl Adapter for TcpAdapter {
    fn split(self: Box<Self>) -> (DynRead, DynWrite) {
        let (r, w) = self.stream.into_split();
        (Box::new(r), Box::new(w))
    }
}

// ---------------------------------------------------------- AsyncStdioAdapter

/// Adapter that wraps a `tokio::process::Child`'s stdin/stdout. NEW —
/// distinct from the legacy synchronous `ChildStdio`. The child is killed
/// when this adapter is dropped (after `split`, the `Child` handle moves
/// into a guard returned alongside, but we accept that the writer half
/// owning the child via a wrapper would be more involved; for Phase 1 we
/// simply spawn the child without a guard and rely on the child exiting
/// when its stdin is closed).
pub struct AsyncStdioAdapter {
    stdin: ChildStdin,
    stdout: ChildStdout,
    _child: Child,
}

impl AsyncStdioAdapter {
    pub fn new(mut child: Child) -> Result<Self, AdapterError> {
        let stdin = child.stdin.take().ok_or_else(|| {
            AdapterError::Other("child stdin missing".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            AdapterError::Other("child stdout missing".into())
        })?;
        Ok(Self { stdin, stdout, _child: child })
    }
}

impl Adapter for AsyncStdioAdapter {
    fn split(self: Box<Self>) -> (DynRead, DynWrite) {
        // Leak the child handle into the writer side via a struct that
        // owns both. Cleaner than dropping it on the floor: when the
        // writer is dropped, so is the child, killing any orphan.
        struct WriterWithChild {
            inner: ChildStdin,
            _child: Child,
        }
        impl AsyncWrite for WriterWithChild {
            fn poll_write(
                mut self: Pin<&mut Self>,
                cx: &mut TaskContext<'_>,
                buf: &[u8],
            ) -> Poll<std::io::Result<usize>> {
                Pin::new(&mut self.inner).poll_write(cx, buf)
            }
            fn poll_flush(
                mut self: Pin<&mut Self>,
                cx: &mut TaskContext<'_>,
            ) -> Poll<std::io::Result<()>> {
                Pin::new(&mut self.inner).poll_flush(cx)
            }
            fn poll_shutdown(
                mut self: Pin<&mut Self>,
                cx: &mut TaskContext<'_>,
            ) -> Poll<std::io::Result<()>> {
                Pin::new(&mut self.inner).poll_shutdown(cx)
            }
        }
        let writer = WriterWithChild { inner: self.stdin, _child: self._child };
        (Box::new(self.stdout), Box::new(writer))
    }
}

// ------------------------------------------------------------- InprocAdapter

/// Pair of in-process byte channels for tests. `pair()` returns two
/// adapters whose reads/writes are connected: bytes written to `a` arrive
/// when `b` reads, and vice versa.
pub struct InprocAdapter {
    reader: InprocReader,
    writer: InprocWriter,
}

impl InprocAdapter {
    pub fn pair() -> (Self, Self) {
        let (a_to_b_tx, a_to_b_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (b_to_a_tx, b_to_a_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let a = InprocAdapter {
            reader: InprocReader::new(b_to_a_rx),
            writer: InprocWriter::new(a_to_b_tx),
        };
        let b = InprocAdapter {
            reader: InprocReader::new(a_to_b_rx),
            writer: InprocWriter::new(b_to_a_tx),
        };
        (a, b)
    }
}

impl Adapter for InprocAdapter {
    fn split(self: Box<Self>) -> (DynRead, DynWrite) {
        (Box::new(self.reader), Box::new(self.writer))
    }
}

pub struct InprocReader {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    leftover: Vec<u8>,
}

impl InprocReader {
    fn new(rx: mpsc::UnboundedReceiver<Vec<u8>>) -> Self {
        Self { rx, leftover: Vec::new() }
    }
}

impl AsyncRead for InprocReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if !self.leftover.is_empty() {
            let n = std::cmp::min(self.leftover.len(), buf.remaining());
            let tail = self.leftover.split_off(n);
            buf.put_slice(&self.leftover);
            self.leftover = tail;
            return Poll::Ready(Ok(()));
        }
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(bytes)) => {
                let n = std::cmp::min(bytes.len(), buf.remaining());
                buf.put_slice(&bytes[..n]);
                if n < bytes.len() {
                    self.leftover.extend_from_slice(&bytes[n..]);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct InprocWriter {
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl InprocWriter {
    fn new(tx: mpsc::UnboundedSender<Vec<u8>>) -> Self {
        Self { tx }
    }
}

impl AsyncWrite for InprocWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.tx
            .send(buf.to_vec())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "inproc closed"))?;
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
