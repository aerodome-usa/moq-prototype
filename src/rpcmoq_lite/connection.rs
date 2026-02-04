use async_stream::stream;
use bytes::Bytes;
use futures::Stream;
use moq_lite::{BroadcastConsumer, Track, TrackConsumer, TrackProducer};
use prost::Message;
use std::pin::Pin;
use tonic::Status;

use crate::rpcmoq_lite::error::RpcError;

/// A stream of raw bytes from a MoQ track.
///
/// This wraps a `TrackConsumer` and yields frames as `Bytes`.
pub struct RpcInbound {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, Status>> + Send>>,
}

impl RpcInbound {
    /// Create a new inbound stream from a broadcast consumer.
    pub fn new(broadcast: &BroadcastConsumer, track_name: &str) -> Self {
        let track = broadcast.subscribe_track(&Track::new(track_name));
        Self::from_track(track)
    }

    /// Create from an existing track consumer.
    pub fn from_track(mut track: TrackConsumer) -> Self {
        let inner = stream! {
            loop {
                match track.next_group().await {
                    Ok(Some(mut group)) => {
                        while let Ok(Some(frame)) = group.read_frame().await {
                            yield Ok(frame);
                        }
                    }
                    Ok(None) => {
                        // Track closed
                        break;
                    }
                    Err(e) => {
                        yield Err(Status::internal(format!("MoQ error: {e}")));
                        break;
                    }
                }
            }
        };

        Self {
            inner: Box::pin(inner),
        }
    }
}

impl Stream for RpcInbound {
    type Item = Result<Bytes, Status>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// A sink for sending responses back to a MoQ track.
pub struct RpcOutbound {
    track: TrackProducer,
}

impl RpcOutbound {
    /// Create a new outbound sink from a track producer.
    pub fn new(track: TrackProducer) -> Self {
        Self { track }
    }

    /// Send a protobuf message.
    pub fn send<M: Message>(&mut self, msg: &M) -> Result<(), RpcError> {
        let mut buf = Vec::with_capacity(msg.encoded_len());
        msg.encode(&mut buf)?;
        self.send_raw(buf);
        Ok(())
    }

    /// Send raw bytes.
    pub fn send_raw(&mut self, bytes: impl Into<Bytes>) {
        self.track.write_frame(bytes.into());
    }
}
