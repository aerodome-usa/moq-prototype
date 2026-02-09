//! Client-side types for rpcmoq_lite.
//!
//! This module contains the `RpcClient` and related types for building
//! clients that communicate with RPC servers over MoQ.
//!
//! # Example
//!
//! ```ignore
//! use rpcmoq_lite::client::{RpcClient, RpcClientConfig};
//! use futures::{SinkExt, StreamExt};
//!
//! let config = RpcClientConfig::builder()
//!     .client_id("drone-123")
//!     .client_prefix("drone".to_string())
//!     .server_prefix("server".to_string())
//!     .build();
//!
//! let mut client = RpcClient::new(producer, consumer, config);
//!
//! // Connect to an RPC endpoint
//! let conn = client
//!     .connect::<Request, Response>("package.Service/Method")
//!     .await?;
//!
//! // Use as combined Sink + Stream
//! conn.send(request).await?;
//! let response = conn.next().await;
//!
//! // Or split for separate send/receive tasks
//! let (sender, receiver) = conn.split();
//! ```

mod config;
mod connection;
mod rpc_client;

pub use config::RpcClientConfig;
pub use connection::{RpcConnection, RpcReceiver, RpcSender};
pub use rpc_client::RpcClient;
