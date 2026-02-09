use futures::{Stream, StreamExt};
use moq_lite::BroadcastProducer;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tonic::Status;

use crate::connection::{RpcInbound, RpcOutbound};
use crate::error::RpcWireError;
use crate::server::session::SessionGuard;

/// A type-erased handler that can be stored in a HashMap.
///
/// This trait allows us to store handlers with different type parameters
/// in a single registry.
pub(crate) trait ErasedHandler: Send + Sync {
    /// Spawn a task to handle the connection.
    ///
    /// Takes raw bytes from MoQ, decodes them, calls the connector,
    /// encodes responses, and writes them back to MoQ.
    fn spawn_handler(
        &self,
        client_id: String,
        inbound: RpcInbound,
        outbound: RpcOutbound,
        connection_guard: ConnectionGuard,
    );
}

/// A concrete typed inbound stream that decodes protobuf messages from `RpcInbound`.
pub struct DecodedInbound<Req> {
    inner: RpcInbound,
    on_decode_error: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    _marker: PhantomData<fn() -> Req>,
}

impl<Req> DecodedInbound<Req> {
    pub fn new(inner: RpcInbound) -> Self {
        Self {
            inner,
            on_decode_error: None,
            _marker: PhantomData,
        }
    }

    /// Attach a callback that runs when a decode error occurs.
    pub fn with_decode_error_handler<F>(mut self, f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_decode_error = Some(std::sync::Arc::new(f));
        self
    }
}

impl<Req> Stream for DecodedInbound<Req>
where
    Req: prost::Message + Default,
{
    type Item = Req;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => match Req::decode(bytes) {
                Ok(msg) => Poll::Ready(Some(msg)),
                // stop the stream, close the connection if we cannot decode the
                // message
                Err(_) => {
                    if let Some(handler) = &this.on_decode_error {
                        handler();
                    }
                    Poll::Ready(None)
                }
            },
            // if we got an error, close the connection
            Poll::Ready(Some(Err(err))) => {
                tracing::error!(%err, "Got an error from MoQ");
                Poll::Ready(None)
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A connector function that bridges MoQ streams to gRPC.
///
/// The connector receives:
/// - `client_id`: The ID of the connecting client
/// - `inbound`: A stream of decoded request messages from the client
///
/// It should:
/// 1. Connect to the appropriate gRPC service
/// 2. Call the correct RPC method with the inbound stream
/// 3. Return the response stream
pub type ConnectorFn<Req, Resp> = Arc<
    dyn Fn(
            String,
            DecodedInbound<Req>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            Pin<Box<dyn Stream<Item = Result<Resp, Status>> + Send>>,
                            Status,
                        >,
                    > + Send,
            >,
        > + Send
        + Sync
        + 'static,
>;

/// A typed handler that wraps a connector function.
pub(crate) struct TypedHandler<Req, Resp> {
    connector: ConnectorFn<Req, Resp>,
    _marker: std::marker::PhantomData<(Req, Resp)>,
}

impl<Req, Resp> TypedHandler<Req, Resp>
where
    Req: prost::Message + Default + Send,
    Resp: prost::Message + Send,
{
    pub fn new(connector: ConnectorFn<Req, Resp>) -> Self {
        Self {
            connector,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Req, Resp> ErasedHandler for TypedHandler<Req, Resp>
where
    Req: prost::Message + Default + Send + 'static,
    Resp: prost::Message + Send + 'static,
{
    fn spawn_handler(
        &self,
        client_id: String,
        inbound: RpcInbound,
        outbound: RpcOutbound,
        connection_guard: ConnectionGuard,
    ) {
        let connector = Arc::clone(&self.connector);
        let grpc_path = connection_guard.session_guard.grpc_path().to_string();

        tokio::spawn(async move {
            // Keep the session guard alive for the duration of the task
            let _guard = connection_guard;

            // Decode inbound bytes to typed messages with a concrete stream type.
            let abort_outbound = outbound.clone();
            let decode_client_id = client_id.clone();
            let decode_grpc_path = grpc_path.clone();
            let typed_inbound =
                DecodedInbound::<Req>::new(inbound).with_decode_error_handler(move || {
                    tracing::warn!(
                        client_id = %decode_client_id,
                        grpc_path = %decode_grpc_path,
                        "Failed to decode request from client"
                    );
                    abort_outbound.abort_app(RpcWireError::Decode.to_code());
                });

            // Call the connector to get the response stream
            let mut outbound = outbound;

            let response_stream = match connector(client_id.clone(), typed_inbound).await {
                Ok(stream) => stream,
                Err(status) => {
                    tracing::warn!(
                        client_id = %client_id,
                        grpc_path = %grpc_path,
                        error = %status,
                        "Connector failed to establish gRPC connection"
                    );
                    outbound.abort_app(RpcWireError::Grpc.to_code());
                    return;
                }
            };

            // Pipe responses back to MoQ
            let mut response_stream = response_stream;

            while let Some(result) = response_stream.next().await {
                match result {
                    Ok(msg) => {
                        if let Err(e) = outbound.send(&msg) {
                            tracing::warn!(
                                client_id = %client_id,
                                grpc_path = %grpc_path,
                                error = %e,
                                "Failed to send response to MoQ"
                            );
                            outbound.abort_app(RpcWireError::Internal.to_code());
                            return;
                        }
                    }
                    Err(status) => {
                        tracing::warn!(
                            client_id = %client_id,
                            grpc_path = %grpc_path,
                            error = %status,
                            "gRPC response stream error"
                        );
                        outbound.abort_app(RpcWireError::Grpc.to_code());
                        return;
                    }
                }
            }

            tracing::debug!(
                client_id = %client_id,
                grpc_path = %grpc_path,
                "Handler completed"
            );
        });
    }
}

// A guard that keeps relevant pieces of data alive until they need to be dropped.
pub(crate) struct ConnectionGuard {
    // Session guard needs to stay alive for the handler call duration
    pub session_guard: SessionGuard,
    // If we drop the response_broadcast, the broadcast will close
    pub _response_broadcast: BroadcastProducer,
}

/// Helper to create a boxed connector from an async closure.
///
/// This handles the type gymnastics of boxing the closure and its return type.
pub fn make_connector<Req, Resp, F, Fut, S>(f: F) -> ConnectorFn<Req, Resp>
where
    Req: prost::Message + Default + Send,
    Resp: prost::Message + Send,
    F: Fn(String, DecodedInbound<Req>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<S, Status>> + Send + 'static,
    S: Stream<Item = Result<Resp, Status>> + Send + 'static,
{
    Arc::new(move |client_id, inbound| {
        let fut = f(client_id, inbound);
        Box::pin(async move {
            let stream = fut.await?;
            Ok(Box::pin(stream) as Pin<Box<dyn Stream<Item = Result<Resp, Status>> + Send>>)
        })
    })
}
