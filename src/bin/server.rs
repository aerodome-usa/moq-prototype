use anyhow::Result;
use futures::StreamExt as _;
use moq_prototype::PRIMARY_TRACK;
use moq_prototype::connect_bidirectional;
use moq_prototype::drone::DroneSessionMap;
use moq_prototype::drone_proto::DronePosition;
use moq_prototype::grpc::{self, EchoServiceClient};
use moq_prototype::rpcmoq_lite::{RpcRouter, RpcRouterConfig};
use moq_prototype::unit_context::UnitContext;
use moq_prototype::unit_map::UnitMap;
use std::sync::Arc;
use tracing::{error, info};

const GRPC_ADDR: &str = "[::1]:50051";
const GRPC_CLIENT_ADDR: &str = "http://[::1]:50051";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let url = std::env::var("RELAY_URL").unwrap_or_else(|_| "https://localhost:4443".to_string());

    let unit_map: Arc<UnitMap<UnitContext>> = Arc::new(UnitMap::new());
    let session_map: Arc<DroneSessionMap> = Arc::new(DroneSessionMap::new());

    let grpc_addr = GRPC_ADDR.parse()?;
    let server_unit_map = Arc::clone(&unit_map);
    let server_session_map = Arc::clone(&session_map);
    tokio::spawn(async move {
        if let Err(e) = grpc::start_server(grpc_addr, server_unit_map, server_session_map).await {
            error!("gRPC server error: {e}");
        }
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    info!("Server connecting to relay at {url}");

    let (_session, producer, consumer) = connect_bidirectional(&url).await?;
    let producer = Arc::new(producer);

    let config = RpcRouterConfig {
        // FIXME: Currently panics if a broadcast is announced that does not have
        // the prefix. This is being fixed in the library and will work soonTM
        client_prefix: String::from("drone"),
        response_prefix: String::from("server"),
        track_name: String::from(PRIMARY_TRACK),
    };

    let mut router = RpcRouter::new(consumer.clone(), producer.clone(), config);

    router.register::<DronePosition, DronePosition, _, _, _>(
        "drone.EchoService/Echo",
        |_, inbound| async move {
            let inbound = inbound.filter_map(|s| async move { s.ok() });
            let mut client = EchoServiceClient::connect(GRPC_CLIENT_ADDR)
                .await
                .inspect_err(|e| tracing::error!(?e))
                .map_err(|e| tonic::Status::internal(e.to_string()))?;
            let response = client.echo(inbound).await?;
            Ok(response.into_inner())
        },
    )?;

    info!("Waiting for drones to connect...");

    router.run().await?;

    Ok(())
}
