//! Server-side types for rpcmoq_lite.
//!
//! This module contains the `RpcRouter` and related types for building
//! servers that bridge MoQ clients to gRPC backends.

mod config;
mod handler;
mod router;
mod session;

pub use config::RpcRouterConfig;
pub use handler::DecodedInbound;
pub use router::RpcRouter;
pub use session::{SessionGuard, SessionKey, SessionMap};
