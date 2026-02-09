use std::time::Duration;

use bon::Builder;

/// Configuration for the RPC client.
#[derive(Debug, Clone, Builder)]
pub struct RpcClientConfig {
    /// Unique client identifier.
    pub client_id: String,

    /// Optional prefix for client broadcasts (e.g., "drone").
    /// If set, client broadcasts are created at `{client_prefix}/{client_id}/{grpc_path}`.
    /// If not set, broadcasts are created at `{client_id}/{grpc_path}`.
    pub client_prefix: Option<String>,

    /// Optional prefix for server responses (e.g., "server").
    /// If set, client subscribes to server responses at `{server_prefix}/{client_id}/{grpc_path}`.
    /// If not set, subscribes at `{client_id}/{grpc_path}`.
    pub server_prefix: Option<String>,

    /// Track name for RPC messages (e.g., "primary").
    #[builder(default = "primary".to_string())]
    pub track_name: String,

    /// Timeout for waiting for server response broadcast.
    #[builder(default = Duration::from_secs(30))]
    pub timeout: Duration,
}

impl RpcClientConfig {
    /// Build the client broadcast path for a given gRPC path.
    pub(crate) fn client_path(&self, grpc_path: &str) -> String {
        match &self.client_prefix {
            Some(prefix) => format!("{}/{}/{}", prefix, self.client_id, grpc_path),
            None => format!("{}/{}", self.client_id, grpc_path),
        }
    }

    /// Build the expected server response path for a given gRPC path.
    pub(crate) fn server_path(&self, grpc_path: &str) -> String {
        match &self.server_prefix {
            Some(prefix) => format!("{}/{}/{}", prefix, self.client_id, grpc_path),
            None => format!("{}/{}", self.client_id, grpc_path),
        }
    }
}
