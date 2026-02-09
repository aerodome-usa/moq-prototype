use anyhow::Result;
use futures::{SinkExt, StreamExt};
use moq_prototype::PRIMARY_TRACK;
use moq_prototype::connect_bidirectional;
use moq_prototype::drone_proto::DronePosition;
use rpcmoq_lite::{RpcClient, RpcClientConfig};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::interval;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let url = std::env::var("RELAY_URL").unwrap_or_else(|_| "https://localhost:4443".to_string());
    let drone_id = std::env::var("DRONE_ID").unwrap_or_else(|_| Uuid::new_v4().to_string());

    info!(
        drone_id = %drone_id,
        relay = %url,
        "Drone connecting to relay"
    );

    let (_session, producer, consumer) = connect_bidirectional(&url).await?;

    let config = RpcClientConfig::builder()
        .client_id(drone_id.clone())
        .client_prefix("drone".to_string())
        .server_prefix("server".to_string())
        .track_name(PRIMARY_TRACK.to_string())
        .timeout(Duration::from_secs(60))
        .build();

    let mut client = RpcClient::new(Arc::new(producer), consumer, config);

    let conn = client
        .connect::<DronePosition, DronePosition>("drone.EchoService/Echo")
        .await?;

    info!(drone_id = %drone_id, "Drone is online");

    let (mut sender, mut receiver) = conn.split();

    // Spawn a task to send position updates
    let send_drone_id = drone_id.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));

        loop {
            ticker.tick().await;

            let pos = DronePosition {
                drone_id: send_drone_id.clone(),
                latitude: 37.7749,
                longitude: -122.4194,
                altitude_m: 100.0,
                heading_deg: 0.0,
                speed_mps: 0.0,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            };

            if let Err(e) = sender.send(pos).await {
                warn!(error = %e, "Failed to send position, stopping sender");
                break;
            }

            debug!(lat = 37.7749, lon = -122.4194, alt = 100.0, "Sent position");
        }
    });

    // Receive echoed responses in the main task
    while let Some(result) = receiver.next().await {
        match result {
            Ok(_echo) => {
                info!("Received echo");
            }
            Err(e) => {
                warn!(error = %e, "Echo receive error");
            }
        }
    }

    info!("Echo stream closed, drone shutting down");
    Ok(())
}
