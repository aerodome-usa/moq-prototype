use crate::rpcmoq_lite::error::RpcError;

/// A parsed RPC request path: `{client_id}/{grpc_path}`
///
/// Example: `drone-123/drone.EchoService/Echo`
/// - `client_id`: `drone-123`
/// - `grpc_path`: `drone.EchoService/Echo`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcRequestPath {
    pub client_id: String,
    pub grpc_path: GrpcPath,
}

impl RpcRequestPath {
    /// Parse a path string into an RpcRequestPath.
    ///
    /// Expected format: `{client_id}/{package}.{service}/{method}`
    /// The client_id can contain slashes, so we split from the right.
    pub fn parse(path: &str) -> Result<Self, RpcError> {
        let path = path.strip_prefix('/').unwrap_or(path);

        // Split on '/' and work backwards to find the service/method boundary
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() < 2 {
            return Err(RpcError::PathParse(format!(
                "path must have at least client_id and grpc_path: '{path}'"
            )));
        }

        // The method is the last part
        let method = parts[parts.len() - 1];

        // The service (with package) is the second-to-last part, and must contain a '.'
        let service_part = parts[parts.len() - 2];
        if !service_part.contains('.') {
            return Err(RpcError::PathParse(format!(
                "service part must contain package.service: '{service_part}'"
            )));
        }

        // Everything before the service part is the client_id
        let client_id = if parts.len() > 2 {
            parts[..parts.len() - 2].join("/")
        } else {
            return Err(RpcError::PathParse(format!(
                "path must include client_id before grpc path: '{path}'"
            )));
        };

        let grpc_path = GrpcPath::parse(&format!("{service_part}/{method}"))?;

        Ok(RpcRequestPath {
            client_id,
            grpc_path,
        })
    }
}

/// A parsed gRPC method path: `{package}.{service}/{method}`
///
/// Example: `drone.EchoService/Echo`
/// - `package`: `drone`
/// - `service`: `EchoService`
/// - `method`: `Echo`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GrpcPath {
    pub package: String,
    pub service: String,
    pub method: String,
}

impl GrpcPath {
    /// Parse a gRPC path string.
    ///
    /// Expected format: `{package}.{service}/{method}`
    pub fn parse(path: &str) -> Result<Self, RpcError> {
        let path = path.strip_prefix('/').unwrap_or(path);

        let (service_path, method) = path
            .rsplit_once('/')
            .ok_or_else(|| RpcError::PathParse(format!("gRPC path must contain '/': '{path}'")))?;

        let (package, service) = service_path.rsplit_once('.').ok_or_else(|| {
            RpcError::PathParse(format!(
                "service path must contain package.service: '{service_path}'"
            ))
        })?;

        if package.is_empty() || service.is_empty() || method.is_empty() {
            return Err(RpcError::PathParse(format!(
                "package, service, and method must all be non-empty: '{path}'"
            )));
        }

        Ok(GrpcPath {
            package: package.to_owned(),
            service: service.to_owned(),
            method: method.to_owned(),
        })
    }

    /// Returns the full service name: `{package}.{service}`
    pub fn full_service(&self) -> String {
        format!("{}.{}", self.package, self.service)
    }

    /// Returns the full gRPC path: `{package}.{service}/{method}`
    pub fn full_path(&self) -> String {
        format!("{}.{}/{}", self.package, self.service, self.method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_path_parse() {
        let path = GrpcPath::parse("drone.EchoService/Echo").unwrap();
        assert_eq!(path.package, "drone");
        assert_eq!(path.service, "EchoService");
        assert_eq!(path.method, "Echo");
        assert_eq!(path.full_service(), "drone.EchoService");
        assert_eq!(path.full_path(), "drone.EchoService/Echo");
    }

    #[test]
    fn test_grpc_path_with_leading_slash() {
        let path = GrpcPath::parse("/drone.EchoService/Echo").unwrap();
        assert_eq!(path.package, "drone");
        assert_eq!(path.service, "EchoService");
        assert_eq!(path.method, "Echo");
    }

    #[test]
    fn test_grpc_path_nested_package() {
        let path = GrpcPath::parse("com.example.drone.EchoService/Echo").unwrap();
        assert_eq!(path.package, "com.example.drone");
        assert_eq!(path.service, "EchoService");
        assert_eq!(path.method, "Echo");
    }

    #[test]
    fn test_rpc_request_path_parse() {
        let path = RpcRequestPath::parse("drone-123/drone.EchoService/Echo").unwrap();
        assert_eq!(path.client_id, "drone-123");
        assert_eq!(path.grpc_path.package, "drone");
        assert_eq!(path.grpc_path.service, "EchoService");
        assert_eq!(path.grpc_path.method, "Echo");
    }

    #[test]
    fn test_rpc_request_path_with_slashes_in_client_id() {
        let path = RpcRequestPath::parse("region/fleet/drone-123/drone.EchoService/Echo").unwrap();
        assert_eq!(path.client_id, "region/fleet/drone-123");
        assert_eq!(path.grpc_path.full_path(), "drone.EchoService/Echo");
    }

    #[test]
    fn test_rpc_request_path_missing_client_id() {
        let result = RpcRequestPath::parse("drone.EchoService/Echo");
        assert!(result.is_err());
    }

    #[test]
    fn test_grpc_path_missing_method() {
        let result = GrpcPath::parse("drone.EchoService");
        assert!(result.is_err());
    }

    #[test]
    fn test_grpc_path_missing_package() {
        let result = GrpcPath::parse("EchoService/Echo");
        assert!(result.is_err());
    }
}
