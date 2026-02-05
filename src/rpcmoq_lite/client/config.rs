use std::time::Duration;

/// Configuration for the RPC client.
#[derive(Debug, Clone)]
pub struct RpcClientConfig {
    /// Prefix for client broadcasts (e.g., "drone").
    /// Client broadcasts are created at `{client_prefix}/{client_id}/{grpc_path}`.
    pub client_prefix: String,

    /// Prefix for server responses (e.g., "server").
    /// Client subscribes to server responses at `{server_prefix}/{client_id}/{grpc_path}`.
    pub server_prefix: String,

    /// Track name for RPC messages (e.g., "primary").
    pub track_name: String,

    /// Unique client identifier.
    pub client_id: String,

    /// Timeout for waiting for server response broadcast.
    pub timeout: Duration,
}

impl Default for RpcClientConfig {
    fn default() -> Self {
        Self {
            client_prefix: "client".to_string(),
            server_prefix: "server".to_string(),
            track_name: "primary".to_string(),
            client_id: String::new(), // Must be set by user
            timeout: Duration::from_secs(30),
        }
    }
}

impl RpcClientConfig {
    /// Create a new config with the given client ID.
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            ..Default::default()
        }
    }

    /// Set the client prefix.
    pub fn with_client_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.client_prefix = prefix.into();
        self
    }

    /// Set the server prefix.
    pub fn with_server_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.server_prefix = prefix.into();
        self
    }

    /// Set the track name.
    pub fn with_track_name(mut self, name: impl Into<String>) -> Self {
        self.track_name = name.into();
        self
    }

    /// Set the timeout for waiting for server response.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Build the client broadcast path for a given gRPC path.
    pub(crate) fn client_path(&self, grpc_path: &str) -> String {
        format!("{}/{}/{}", self.client_prefix, self.client_id, grpc_path)
    }

    /// Build the expected server response path for a given gRPC path.
    pub(crate) fn server_path(&self, grpc_path: &str) -> String {
        format!("{}/{}/{}", self.server_prefix, self.client_id, grpc_path)
    }
}
