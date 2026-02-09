use async_stream::stream;
use bytes::Bytes;
use futures::Stream;
use moq_lite::{BroadcastConsumer, Error as MoqError, Track, TrackConsumer, TrackProducer};
use prost::Message;
use std::pin::Pin;

use crate::rpcmoq_lite::error::RpcSendError;

/// A stream of raw bytes from a MoQ track.
///
/// This wraps a `TrackConsumer` and yields frames as `Bytes`.
pub struct RpcInbound {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, moq_lite::Error>> + Send>>,
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
                        yield Err(e);
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
    type Item = Result<Bytes, moq_lite::Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// A sink for sending responses back to a MoQ track.
#[derive(Clone)]
pub struct RpcOutbound {
    track: TrackProducer,
}

impl RpcOutbound {
    /// Create a new outbound sink from a track producer.
    pub fn new(track: TrackProducer) -> Self {
        Self { track }
    }

    /// Send a protobuf message.
    pub fn send<M: Message>(&mut self, msg: &M) -> Result<(), RpcSendError> {
        let mut buf = Vec::with_capacity(msg.encoded_len());
        msg.encode(&mut buf)?;
        self.send_raw(buf);
        Ok(())
    }

    /// Send raw bytes.
    pub fn send_raw(&mut self, bytes: impl Into<Bytes>) {
        self.track.write_frame(bytes.into());
    }

    /// Abort the underlying track with an application error code.
    pub fn abort_app(&self, code: u32) {
        self.track.clone().abort(MoqError::App(code));
    }
}
