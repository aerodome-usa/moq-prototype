use thiserror::Error;

/// Errors that can occur while parsing RPC or gRPC paths.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RpcPathError {
    #[error("invalid RPC path: {0}")]
    Invalid(String),
}

/// Errors that can occur when establishing or managing an RPC client connection.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RpcClientError {
    /// Failed to create a broadcast for the request channel.
    #[error("failed to create broadcast: {0}")]
    BroadcastCreate(String),

    /// Timeout waiting for server response broadcast.
    #[error("timeout waiting for server response")]
    Timeout(#[from] tokio::time::error::Elapsed),

    /// Server broadcast not found at expected path.
    #[error("server broadcast not found at path: {0}")]
    ServerNotFound(String),

    /// The RPC connection was closed.
    #[error("RPC connection closed")]
    ConnectionClosed,
}

/// Errors that can occur while running the RPC server router.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RpcServerError {
    /// Failed to parse an RPC request path.
    #[error(transparent)]
    Path(#[from] RpcPathError),

    /// A session already exists for this client and RPC path.
    #[error("session already active for client '{client_id}' on '{grpc_path}'")]
    SessionAlreadyActive {
        client_id: String,
        grpc_path: String,
    },

    /// No handler registered for the given gRPC path.
    #[error("no handler registered for '{0}'")]
    NoHandler(String),

    /// Failed to create a broadcast for the response channel.
    #[error("failed to create broadcast: {0}")]
    BroadcastCreate(String),

    /// Authorization failed for the requested operation.
    #[error("unauthorized: {0}")]
    Unauthorized(String),
}

/// Errors that can occur while encoding outbound messages.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RpcSendError {
    /// Failed to encode a protobuf message.
    #[error("protobuf encode error")]
    Encode(#[from] prost::EncodeError),
}

/// Errors that can occur on the wire after a connection is established.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RpcWireError {
    /// No handler registered for the given gRPC path.
    #[error("no handler registered")]
    NoHandler,

    /// A session already exists for this client and RPC path.
    #[error("session already active")]
    SessionAlreadyActive,

    /// Failed to decode a protobuf message.
    #[error("protobuf decode error")]
    Decode,

    /// The gRPC backend returned an error.
    #[error("gRPC error")]
    Grpc,

    /// Internal server error while handling the request.
    #[error("internal error")]
    Internal,

    /// An error from the underlying MoQ transport.
    #[error("MoQ transport error")]
    Transport(#[source] moq_lite::Error),

    /// An unknown MoQ application error code.
    #[error("unknown MoQ app error code: {0}")]
    Unknown(u32),
}

impl RpcWireError {
    pub const CODE_NO_HANDLER: u32 = 1;
    pub const CODE_SESSION_ALREADY_ACTIVE: u32 = 2;
    pub const CODE_DECODE: u32 = 3;
    pub const CODE_GRPC: u32 = 4;
    pub const CODE_INTERNAL: u32 = 5;

    pub fn transport_with(err: moq_lite::Error) -> Self {
        match err {
            moq_lite::Error::App(code) => RpcWireError::from_code(code),
            other => RpcWireError::Transport(other),
        }
    }

    pub fn to_code(&self) -> u32 {
        match self {
            RpcWireError::NoHandler => Self::CODE_NO_HANDLER,
            RpcWireError::SessionAlreadyActive => Self::CODE_SESSION_ALREADY_ACTIVE,
            RpcWireError::Decode => Self::CODE_DECODE,
            RpcWireError::Grpc => Self::CODE_GRPC,
            RpcWireError::Internal => Self::CODE_INTERNAL,
            RpcWireError::Transport(e) => e.to_code(),
            RpcWireError::Unknown(code) => *code,
        }
    }

    pub fn from_code(code: u32) -> Self {
        match code {
            Self::CODE_NO_HANDLER => RpcWireError::NoHandler,
            Self::CODE_SESSION_ALREADY_ACTIVE => RpcWireError::SessionAlreadyActive,
            Self::CODE_DECODE => RpcWireError::Decode,
            Self::CODE_GRPC => RpcWireError::Grpc,
            Self::CODE_INTERNAL => RpcWireError::Internal,
            // TODO: Go implement from_code in the moq-lite codebase
            other => RpcWireError::Unknown(other),
        }
    }
}

impl From<moq_lite::Error> for RpcWireError {
    fn from(err: moq_lite::Error) -> Self {
        RpcWireError::transport_with(err)
    }
}
