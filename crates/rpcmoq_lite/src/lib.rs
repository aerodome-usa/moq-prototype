//! # rpcmoq_lite
//!
//! A library for bridging bidirectional gRPC streaming over MoQ (Media over QUIC).
//!
//! This library provides both client and server components:
//!
//! ## Server Side
//!
//! The `RpcRouter` listens for client announcements and routes connections to
//! registered handlers that bridge to gRPC backends.
//!
//! ```ignore
//! use rpcmoq_lite::{RpcRouter, RpcRouterConfig, DecodedInbound};
//!
//! let mut router = RpcRouter::new(consumer, producer, RpcRouterConfig::builder().build());
//!
//! router.register::<Request, Response, _, _, _>(
//!     "package.Service/Method",
//!     |client_id, inbound| async move {
//!         let mut client = GrpcServiceClient::connect(addr).await?;
//!         let response = client.method(inbound.into_ok_stream()).await?;
//!         Ok(response.into_inner())
//!     },
//! )?;
//!
//! router.run().await?;
//! ```
//!
//! ## Client Side
//!
//! The `RpcClient` provides a simple API for connecting to RPC endpoints.
//! Connections implement `Sink` and `Stream` and can be split for concurrent use.
//!
//! ```ignore
//! use rpcmoq_lite::{RpcClient, RpcClientConfig};
//! use futures::{SinkExt, StreamExt};
//!
//! let config = RpcClientConfig::builder()
//!     .client_id("client-123")
//!     .client_prefix("drone".to_string())
//!     .server_prefix("server".to_string())
//!     .build();
//!
//! let mut client = RpcClient::new(producer, consumer, config);
//!
//! let conn = client
//!     .connect::<Request, Response>("package.Service/Method")
//!     .await?;
//!
//! // Split for separate send/receive
//! let (mut sender, mut receiver) = conn.split();
//!
//! sender.send(request).await?;
//! while let Some(response) = receiver.next().await {
//!     println!("{:?}", response?);
//! }
//! ```
//!
//! ## Path Format
//!
//! When prefixes are set:
//! - Client announces at: `{client_prefix}/{client_id}/{package}.{service}/{method}`
//! - Server responds at: `{server_prefix}/{client_id}/{package}.{service}/{method}`
//!
//! When prefixes are omitted:
//! - Client announces at: `{client_id}/{package}.{service}/{method}`
//! - Server responds at: `{client_id}/{package}.{service}/{method}`
//!
//! Example with prefixes:
//! - Client announces: `drone/drone-123/drone.EchoService/Echo`
//! - Server responds: `server/drone-123/drone.EchoService/Echo`
//!
//! Example without prefixes:
//! - Client announces: `drone-123/drone.EchoService/Echo`
//! - Server responds: `drone-123/drone.EchoService/Echo`

// Shared modules at root level
mod connection;
mod error;
mod path;

// Submodules for client and server
pub mod client;
pub mod server;

// Re-export shared types
pub use connection::{RpcInbound, RpcOutbound};
pub use error::{RpcClientError, RpcPathError, RpcSendError, RpcServerError, RpcWireError};
pub use path::{GrpcPath, RpcRequestPath};

// Convenience re-exports for common use
pub use client::{RpcClient, RpcClientConfig, RpcConnection, RpcReceiver, RpcSender};
pub use server::{DecodedInbound, RpcRouter, RpcRouterConfig, SessionGuard, SessionKey, SessionMap};
