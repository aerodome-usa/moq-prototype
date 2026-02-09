use futures::{Sink, Stream};
use moq_lite::BroadcastProducer;
use prost::Message;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::rpcmoq_lite::connection::{RpcInbound, RpcOutbound};
use crate::rpcmoq_lite::error::{RpcSendError, RpcWireError};

/// A bidirectional RPC connection.
///
/// Implements both `Sink` (for sending requests) and `Stream` (for receiving responses).
/// Can be split into separate `RpcSender` and `RpcReceiver` halves using the `split()` method.
///
/// # Example
///
/// ```ignore
/// // Use as a combined sink/stream
/// conn.send(request).await?;
/// while let Some(response) = conn.next().await {
///     println!("Got: {:?}", response?);
/// }
///
/// // Or split for concurrent send/receive
/// let (sender, receiver) = conn.split();
/// tokio::spawn(async move {
///     sender.send(request).await.unwrap();
/// });
/// while let Some(response) = receiver.next().await {
///     println!("Got: {:?}", response?);
/// }
/// ```
pub struct RpcConnection<Req, Resp> {
    sender: RpcSender<Req>,
    receiver: RpcReceiver<Resp>,
}

impl<Req, Resp> RpcConnection<Req, Resp> {
    /// Create a new RPC connection from its parts.
    pub(crate) fn new(
        outbound: RpcOutbound,
        inbound: RpcInbound,
        broadcast: Arc<BroadcastProducer>,
    ) -> Self {
        Self {
            sender: RpcSender::new(outbound, Arc::clone(&broadcast)),
            receiver: RpcReceiver::new(inbound, broadcast),
        }
    }

    /// Split the connection into separate send and receive halves.
    ///
    /// Both halves share ownership of the underlying broadcast, so the connection
    /// stays alive as long as either half is alive.
    pub fn split(self) -> (RpcSender<Req>, RpcReceiver<Resp>) {
        (self.sender, self.receiver)
    }
}

impl<Req, Resp> Stream for RpcConnection<Req, Resp>
where
    Resp: Message + Default,
{
    type Item = Result<Resp, RpcWireError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.receiver).poll_next(cx)
    }
}

impl<Req, Resp> Sink<Req> for RpcConnection<Req, Resp>
where
    Req: Message,
{
    type Error = RpcSendError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sender).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Req) -> Result<(), Self::Error> {
        Pin::new(&mut self.sender).start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sender).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sender).poll_close(cx)
    }
}

/// The send half of an `RpcConnection`.
///
/// Implements `Sink` for sending request messages to the server.
/// Shares ownership of the underlying broadcast with `RpcReceiver`.
pub struct RpcSender<Req> {
    outbound: RpcOutbound,
    // Keeps the broadcast alive; shared with RpcReceiver when split
    _broadcast: Arc<BroadcastProducer>,
    _marker: PhantomData<fn(Req)>,
}

impl<Req> RpcSender<Req> {
    fn new(outbound: RpcOutbound, broadcast: Arc<BroadcastProducer>) -> Self {
        Self {
            outbound,
            _broadcast: broadcast,
            _marker: PhantomData,
        }
    }
}

impl<Req> Sink<Req> for RpcSender<Req>
where
    Req: Message,
{
    type Error = RpcSendError;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // MoQ tracks are always ready to accept writes
        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: Req) -> Result<(), Self::Error> {
        self.outbound.send(&item)?;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // MoQ writes are immediately flushed
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Closing is handled by dropping the broadcast
        Poll::Ready(Ok(()))
    }
}

/// The receive half of an `RpcConnection`.
///
/// Implements `Stream` for receiving response messages from the server.
/// Shares ownership of the underlying broadcast with `RpcSender`.
pub struct RpcReceiver<Resp> {
    inbound: RpcInbound,
    // Keeps the broadcast alive; shared with RpcSender when split
    _broadcast: Arc<BroadcastProducer>,
    _marker: PhantomData<fn() -> Resp>,
}

impl<Resp> RpcReceiver<Resp> {
    fn new(inbound: RpcInbound, broadcast: Arc<BroadcastProducer>) -> Self {
        Self {
            inbound,
            _broadcast: broadcast,
            _marker: PhantomData,
        }
    }
}

impl<Resp> Stream for RpcReceiver<Resp>
where
    Resp: Message + Default,
{
    type Item = Result<Resp, RpcWireError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inbound).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => match Resp::decode(bytes) {
                Ok(msg) => Poll::Ready(Some(Ok(msg))),
                Err(_) => Poll::Ready(Some(Err(RpcWireError::Decode))),
            },
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(RpcWireError::from(err)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
