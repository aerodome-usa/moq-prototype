use futures::Stream;
use moq_lite::{BroadcastConsumer, OriginConsumer, OriginProducer, Track};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tonic::Status;
use tracing::{debug, info, warn};

use crate::connection::{RpcInbound, RpcOutbound};
use crate::error::{RpcServerError, RpcWireError};
use crate::path::RpcRequestPath;
use crate::server::config::RpcRouterConfig;
use crate::server::handler::{
    ConnectionGuard, DecodedInbound, ErasedHandler, TypedHandler, make_connector,
};
use crate::server::session::{SessionKey, SessionMap};

/// The main RPC router that manages connections and dispatches to handlers.
pub struct RpcRouter {
    consumer: OriginConsumer,
    producer: Arc<OriginProducer>,
    sessions: Arc<SessionMap>,
    handlers: HashMap<String, Arc<dyn ErasedHandler>>,
    config: RpcRouterConfig,
}

impl RpcRouter {
    /// Create a new RPC router.
    pub fn new(
        consumer: OriginConsumer,
        producer: Arc<OriginProducer>,
        config: RpcRouterConfig,
    ) -> Self {
        Self {
            consumer,
            producer,
            sessions: Arc::new(SessionMap::new()),
            handlers: HashMap::new(),
            config,
        }
    }

    /// Register a handler for a specific gRPC path.
    ///
    /// # Example
    /// ```ignore
    /// router.register::<DronePosition, DronePosition, _, _, _>(
    ///     "drone.EchoService/Echo",
    ///     |client_id, inbound| async move {
    ///         let mut client = EchoServiceClient::connect(GRPC_ADDR).await
    ///             .map_err(|e| tonic::Status::internal(e.to_string()))?;
    ///         let response = client.echo(inbound.into_ok_stream()).await?;
    ///         Ok(response.into_inner())
    ///     },
    /// )?;
    /// ```
    pub fn register<Req, Resp, F, Fut, S>(
        &mut self,
        grpc_path: impl Into<String>,
        connector: F,
    ) -> Result<(), RpcServerError>
    where
        Req: prost::Message + Default + Send + 'static,
        Resp: prost::Message + Send + 'static,
        F: Fn(String, DecodedInbound<Req>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<S, Status>> + Send + 'static,
        S: Stream<Item = Result<Resp, Status>> + Send + 'static,
    {
        let grpc_path = grpc_path.into();
        let boxed_connector = make_connector(connector);
        let handler = TypedHandler::<Req, Resp>::new(boxed_connector);
        self.handlers.insert(grpc_path.clone(), Arc::new(handler));

        info!(grpc_path = %grpc_path, "Registered RPC handler");
        Ok(())
    }

    /// Run the router, processing connections until shutdown.
    ///
    /// This method consumes the router and runs until the consumer is closed
    /// or a fatal error occurs. Handler tasks continue to run independently.
    pub async fn run(self) -> Result<(), RpcServerError> {
        // Extract fields we need before consuming consumer
        let producer = self.producer;
        let sessions = self.sessions;
        let handlers = self.handlers;
        let config = self.config;

        let mut announcements = match &config.client_prefix {
            Some(prefix) => self.consumer.with_root(prefix).ok_or_else(|| {
                RpcServerError::Unauthorized(format!("prefix '{prefix}' not authorized"))
            })?,
            None => self.consumer,
        };

        info!(
            prefix = ?config.client_prefix,
            "RPC router started, listening for announcements"
        );

        loop {
            match announcements.announced().await {
                Some((path, Some(broadcast))) => {
                    let path_str = path.to_string();
                    debug!(path = %path_str, "Received announcement");

                    if let Err(e) = Self::handle_announcement(
                        &producer, &sessions, &handlers, &config, &path_str, broadcast,
                    ) {
                        warn!(path = %path_str, error = %e, "Failed to handle announcement");
                    }
                }

                Some((path, None)) => {
                    debug!(path = %path.to_string(), "Client disconnected");
                    // Session cleanup happens automatically via SessionGuard drop
                }

                None => {
                    info!("Announcement stream closed, router shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle a new client announcement.
    fn handle_announcement(
        producer: &Arc<OriginProducer>,
        sessions: &Arc<SessionMap>,
        handlers: &HashMap<String, Arc<dyn ErasedHandler>>,
        config: &RpcRouterConfig,
        path: &str,
        broadcast: BroadcastConsumer,
    ) -> Result<(), RpcServerError> {
        let (client_id, grpc_path) = match RpcRequestPath::parse(path) {
            Ok(request_path) => (
                request_path.client_id.clone(),
                request_path.grpc_path.full_path(),
            ),
            Err(e) => return Err(e.into()),
        };

        // Create the response broadcast early so we can surface errors like "no handler".
        let response_path = config.response_path(&client_id, &grpc_path);
        let mut response_broadcast =
            producer.create_broadcast(&response_path).ok_or_else(|| {
                RpcServerError::BroadcastCreate(format!(
                    "failed to create response broadcast at '{response_path}'"
                ))
            })?;

        let outbound_track = response_broadcast.create_track(Track::new(&config.track_name));
        let outbound = RpcOutbound::new(outbound_track);

        let handler = handlers.get(&grpc_path).ok_or_else(|| {
            warn!(
                client_id = %client_id,
                grpc_path = %grpc_path,
                "No handler registered for gRPC path"
            );
            outbound.abort_app(RpcWireError::NoHandler.to_code());
            RpcServerError::NoHandler(grpc_path.clone())
        })?;

        // Try to create a session (prevents duplicate connections)
        let session_key = SessionKey::new(&client_id, &grpc_path);
        let session_guard = match sessions.try_create(session_key) {
            Ok(guard) => guard,
            Err(e @ RpcServerError::SessionAlreadyActive { .. }) => {
                outbound.abort_app(RpcWireError::SessionAlreadyActive.to_code());
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        let inbound = RpcInbound::new(&broadcast, &config.track_name);

        info!(
            client_id = %client_id,
            grpc_path = %grpc_path,
            response_path = %response_path,
            "Spawning handler for new connection"
        );

        let connection_guard = ConnectionGuard {
            session_guard,
            _response_broadcast: response_broadcast,
        };

        handler.spawn_handler(client_id, inbound, outbound, connection_guard);

        Ok(())
    }

    /// Get the number of active sessions.
    pub fn active_sessions(&self) -> usize {
        self.sessions.len()
    }

    /// Check if a handler is registered for the given path.
    pub fn has_handler(&self, grpc_path: &str) -> bool {
        self.handlers.contains_key(grpc_path)
    }
}
