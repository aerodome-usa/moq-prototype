use anyhow::anyhow;

pub struct Path {
    client_identifier: String,
    grpc_path: GrpcPath,
}

impl Path {
    pub fn parse(path: String) -> anyhow::Result<Self> {
        let path = path.strip_prefix('/').unwrap_or(&path);

        let (client_id, grpc_path) = path
            .rsplit_once('/')
            .ok_or(anyhow!("Failed to split on the last slash"))?;

        let grpc_path = GrpcPath::parse(grpc_path)?;

        Ok(Path {
            client_identifier: client_id.to_owned(),
            grpc_path,
        })
    }
}

pub struct GrpcPath {
    package: String,
    service: String,
    method: String,
}

impl GrpcPath {
    pub fn parse(path: &str) -> anyhow::Result<Self> {
        let path = path.strip_prefix('/').unwrap_or(path);

        let (service_path, method) = path
            .rsplit_once('/')
            .ok_or(anyhow!("Failed to split on the last slash"))?;

        let (package, service) = service_path
            .rsplit_once('.')
            .ok_or(anyhow!("Failed to split on the last ."))?;

        Ok(GrpcPath {
            package: package.to_owned(),
            service: service.to_owned(),
            method: method.to_owned(),
        })
    }

    pub fn full_service(&self) -> String {
        format!("{}.{}", self.package, self.service)
    }
}
