/// Configuration for the RPC router.
#[derive(Debug, Clone)]
pub struct RpcRouterConfig {
    /// Prefix for client announcements (e.g., "drone").
    /// The router listens for announcements under this prefix.
    pub client_prefix: String,

    /// Prefix for server responses (e.g., "server").
    /// Responses are published at `{response_prefix}/{client_id}/{grpc_path}`.
    pub response_prefix: String,

    /// Track name for RPC messages (e.g., "primary").
    pub track_name: String,
}

impl Default for RpcRouterConfig {
    fn default() -> Self {
        Self {
            client_prefix: "client".to_string(),
            response_prefix: "server".to_string(),
            track_name: "primary".to_string(),
        }
    }
}
