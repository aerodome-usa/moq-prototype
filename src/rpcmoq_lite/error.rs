use std::fmt;

/// Errors that can occur in the RPC-over-MoQ library.
#[derive(Debug)]
pub enum RpcError {
    /// Failed to parse an RPC request path.
    PathParse(String),

    /// A session already exists for this client and RPC path.
    SessionAlreadyActive {
        client_id: String,
        grpc_path: String,
    },

    /// Failed to create a broadcast for the response channel.
    BroadcastCreate(String),

    /// Failed to encode a protobuf message.
    Encode(prost::EncodeError),

    /// Failed to decode a protobuf message.
    Decode(prost::DecodeError),

    /// An error from the underlying MoQ transport.
    Moq(String),

    /// A handler panicked during execution.
    HandlerPanic,

    /// No handler registered for the given gRPC path.
    NoHandler(String),

    /// gRPC connection or call failed.
    Grpc(tonic::Status),

    /// Timeout waiting for server response broadcast.
    Timeout,

    /// Server broadcast not found at expected path.
    ServerNotFound(String),

    /// The RPC connection was closed.
    ConnectionClosed,
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcError::PathParse(msg) => write!(f, "failed to parse RPC path: {msg}"),
            RpcError::SessionAlreadyActive {
                client_id,
                grpc_path,
            } => write!(
                f,
                "session already active for client '{client_id}' on '{grpc_path}'"
            ),
            RpcError::BroadcastCreate(msg) => write!(f, "failed to create broadcast: {msg}"),
            RpcError::Encode(e) => write!(f, "protobuf encode error: {e}"),
            RpcError::Decode(e) => write!(f, "protobuf decode error: {e}"),
            RpcError::Moq(msg) => write!(f, "MoQ error: {msg}"),
            RpcError::HandlerPanic => write!(f, "handler panicked"),
            RpcError::NoHandler(path) => write!(f, "no handler registered for '{path}'"),
            RpcError::Grpc(status) => write!(f, "gRPC error: {status}"),
            RpcError::Timeout => write!(f, "timeout waiting for server response broadcast"),
            RpcError::ServerNotFound(path) => {
                write!(f, "server broadcast not found at path: {path}")
            }
            RpcError::ConnectionClosed => write!(f, "RPC connection closed"),
        }
    }
}

impl std::error::Error for RpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RpcError::Encode(e) => Some(e),
            RpcError::Decode(e) => Some(e),
            _ => None,
        }
    }
}

impl From<prost::EncodeError> for RpcError {
    fn from(e: prost::EncodeError) -> Self {
        RpcError::Encode(e)
    }
}

impl From<prost::DecodeError> for RpcError {
    fn from(e: prost::DecodeError) -> Self {
        RpcError::Decode(e)
    }
}

impl From<tonic::Status> for RpcError {
    fn from(status: tonic::Status) -> Self {
        RpcError::Grpc(status)
    }
}
