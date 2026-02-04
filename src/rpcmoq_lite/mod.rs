//! # rpcmoq_lite
//!
//! A library for bridging bidirectional gRPC streaming over MoQ (Media over QUIC).
//!
//! This library provides a managed router that:
//! - Listens for client announcements on MoQ
//! - Routes connections to registered handlers based on gRPC path
//! - Manages sessions (preventing duplicate connections)
//! - Spawns and tracks handler tasks
//! - Automatically cleans up on disconnect or error
//!
//! ## Example
//!
//! ```ignore
//! use moq_prototype::rpcmoq_lite::{
//!     BoxedInboundStream, BoxedOutboundStream,
//!     RpcConnector, RpcRouter, RpcRouterConfig,
//! };
//!
//! struct EchoConnector;
//!
//! #[tonic::async_trait]
//! impl RpcConnector for EchoConnector {
//!     type Req = DronePosition;
//!     type Resp = DronePosition;
//!
//!     async fn connect(
//!         &self,
//!         _client_id: String,
//!         inbound: BoxedInboundStream<Self::Req>,
//!     ) -> Result<BoxedOutboundStream<Self::Resp>, tonic::Status> {
//!         let mut client = EchoServiceClient::connect(GRPC_ADDR).await
//!             .map_err(|e| tonic::Status::internal(e.to_string()))?;
//!         let response = client.echo(inbound).await?;
//!         Ok(Box::pin(response.into_inner()))
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let (_session, producer, consumer) = connect_bidirectional(&url).await?;
//!     let producer = Arc::new(producer);
//!
//!     let mut router = RpcRouter::new(consumer, producer, RpcRouterConfig::default());
//!     router.register("drone.EchoService/Echo", EchoConnector)?;
//!     router.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Path Format
//!
//! Clients announce at: `{client_prefix}{client_id}/{package}.{service}/{method}`
//!
//! Server responds at: `{response_prefix}{client_id}/{package}.{service}/{method}`
//!
//! Example:
//! - Client announces: `drone/drone-123/drone.EchoService/Echo`
//! - Server responds: `server/drone-123/drone.EchoService/Echo`

mod connection;
mod error;
mod handler;
mod path;
mod router;
mod session;

// Public API
pub use error::RpcError;
pub use handler::DecodedInbound;
pub use path::{GrpcPath, RpcRequestPath};
pub use router::{RpcRouter, RpcRouterConfig};

// Re-export for convenience in handlers
pub use connection::{RpcInbound, RpcOutbound};
pub use session::{SessionGuard, SessionKey, SessionMap};
