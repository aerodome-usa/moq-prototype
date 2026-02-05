use moq_lite::{BroadcastConsumer, OriginConsumer, OriginProducer, Track};
use prost::Message;
use std::sync::Arc;
use tracing::{debug, info};

use crate::rpcmoq_lite::client::config::RpcClientConfig;
use crate::rpcmoq_lite::client::connection::RpcConnection;
use crate::rpcmoq_lite::connection::{RpcInbound, RpcOutbound};
use crate::rpcmoq_lite::error::RpcError;

/// An RPC client that connects to a server over MoQ.
///
/// The client handles:
/// - Creating broadcasts for sending requests
/// - Waiting for server response broadcasts (with timeout)
/// - Encoding/decoding protobuf messages
///
/// # Example
///
/// ```ignore
/// use rpcmoq_lite::client::{RpcClient, RpcClientConfig};
///
/// let config = RpcClientConfig::new("drone-123")
///     .with_client_prefix("drone")
///     .with_server_prefix("server");
///
/// let mut client = RpcClient::new(producer, consumer, config);
///
/// // Connect returns a bidirectional connection
/// let conn = client
///     .connect::<DronePosition, DronePosition>("drone.EchoService/Echo")
///     .await?;
///
/// // Split for separate send/receive tasks
/// let (mut sender, mut receiver) = conn.split();
///
/// // Send requests
/// sender.send(position).await?;
///
/// // Receive responses
/// while let Some(response) = receiver.next().await {
///     println!("Got: {:?}", response?);
/// }
/// ```
pub struct RpcClient {
    producer: Arc<OriginProducer>,
    consumer: OriginConsumer,
    config: RpcClientConfig,
}

impl RpcClient {
    /// Create a new RPC client.
    pub fn new(
        producer: Arc<OriginProducer>,
        consumer: OriginConsumer,
        config: RpcClientConfig,
    ) -> Self {
        Self {
            producer,
            consumer,
            config,
        }
    }

    /// Connect to an RPC endpoint, returning a bidirectional connection.
    ///
    /// This method:
    /// 1. Creates a broadcast at `{client_prefix}/{client_id}/{grpc_path}`
    /// 2. Waits for the server to announce its response broadcast (with timeout)
    /// 3. Returns an `RpcConnection` that implements `Sink` and `Stream`
    ///
    /// # Type Parameters
    ///
    /// * `Req` - The request message type (must implement `prost::Message`)
    /// * `Resp` - The response message type (must implement `prost::Message + Default`)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// * Failed to create the client broadcast
    /// * Timeout waiting for server response broadcast
    /// * Server broadcast was not found
    pub async fn connect<Req, Resp>(
        &mut self,
        grpc_path: impl Into<String>,
    ) -> Result<RpcConnection<Req, Resp>, RpcError>
    where
        Req: Message + Default + Send + 'static,
        Resp: Message + Default + Send + 'static,
    {
        let grpc_path = grpc_path.into();
        let client_path = self.config.client_path(&grpc_path);
        let server_path = self.config.server_path(&grpc_path);

        info!(
            client_id = %self.config.client_id,
            client_path = %client_path,
            server_path = %server_path,
            "Connecting to RPC endpoint"
        );

        let mut broadcast = self
            .producer
            .create_broadcast(&client_path)
            .ok_or_else(|| {
                RpcError::BroadcastCreate(format!(
                    "failed to create client broadcast at '{client_path}'"
                ))
            })?;

        // Create the outbound track for sending requests
        let outbound_track = broadcast.create_track(Track::new(&self.config.track_name));
        let outbound = RpcOutbound::new(outbound_track);

        let server_broadcast = self.wait_for_server(&server_path).await?;

        // Subscribe to the server's response track
        let inbound = RpcInbound::new(&server_broadcast, &self.config.track_name);

        info!(
            client_id = %self.config.client_id,
            grpc_path = %grpc_path,
            "RPC connection established"
        );

        // Wrap the broadcast in Arc for shared ownership when split
        let broadcast = Arc::new(broadcast);

        Ok(RpcConnection::new(outbound, inbound, broadcast))
    }

    /// Wait for the server to announce its response broadcast.
    async fn wait_for_server(&mut self, server_path: &str) -> Result<BroadcastConsumer, RpcError> {
        let timeout = self.config.timeout;

        debug!(
            server_path = %server_path,
            timeout_secs = %timeout.as_secs(),
            "Waiting for server response broadcast"
        );

        let wait_fut = async {
            loop {
                match self.consumer.announced().await {
                    Some((path, Some(broadcast))) if path.as_str() == server_path => {
                        debug!(path = %server_path, "Found server response broadcast");
                        return Ok(broadcast);
                    }
                    Some((path, None)) if path.as_str() == server_path => {
                        return Err(RpcError::ServerNotFound(server_path.to_string()));
                    }
                    Some(_) => {
                        // Not our path, keep waiting
                        continue;
                    }
                    None => {
                        return Err(RpcError::ConnectionClosed);
                    }
                }
            }
        };

        tokio::time::timeout(timeout, wait_fut).await?
    }

    /// Get the client ID.
    pub fn client_id(&self) -> &str {
        &self.config.client_id
    }

    /// Get the client configuration.
    pub fn config(&self) -> &RpcClientConfig {
        &self.config
    }
}
