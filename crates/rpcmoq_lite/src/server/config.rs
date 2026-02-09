use bon::Builder;

/// Configuration for the RPC router.
#[derive(Debug, Clone, Builder)]
pub struct RpcRouterConfig {
    /// Optional prefix for client announcements (e.g., "drone").
    /// If set, the router listens for announcements under this prefix.
    /// If not set, listens at the root level.
    pub client_prefix: Option<String>,

    /// Optional prefix for server responses (e.g., "server").
    /// If set, responses are published at `{response_prefix}/{client_id}/{grpc_path}`.
    /// If not set, responses are published at `{client_id}/{grpc_path}`.
    pub response_prefix: Option<String>,

    /// Track name for RPC messages (e.g., "primary").
    #[builder(default = "primary".to_string())]
    pub track_name: String,
}

impl RpcRouterConfig {
    /// Build the response path for a client/rpc combination.
    pub(crate) fn response_path(&self, client_id: &str, grpc_path: &str) -> String {
        match &self.response_prefix {
            Some(prefix) => format!("{}/{}/{}", prefix, client_id, grpc_path),
            None => format!("{}/{}", client_id, grpc_path),
        }
    }
}
