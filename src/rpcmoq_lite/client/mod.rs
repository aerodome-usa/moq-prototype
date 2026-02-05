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
//! let config = RpcClientConfig::new("drone-123")
//!     .with_client_prefix("drone")
//!     .with_server_prefix("server");
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
