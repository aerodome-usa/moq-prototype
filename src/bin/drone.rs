use anyhow::Result;
use moq_lite::Track;
use moq_prototype::drone_proto::{self, DronePosition};
use moq_prototype::{
    connect_bidirectional, control_broadcast_path, drone_broadcast_path, COMMAND_TRACK,
    POSITION_TRACK,
};
use prost::Message;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::interval;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("RELAY_URL").unwrap_or_else(|_| "https://localhost:4443".to_string());
    let drone_id = std::env::var("DRONE_ID").unwrap_or_else(|_| Uuid::new_v4().to_string());

    let drone_path = drone_broadcast_path(&drone_id);
    let control_path = control_broadcast_path(&drone_id);

    println!("Drone {drone_id} connecting to relay at {url}");
    println!("  Publishing position on: {drone_path}/{POSITION_TRACK}");
    println!("  Listening for commands on: {control_path}/{COMMAND_TRACK}");

    let (_session, producer, consumer) = connect_bidirectional(&url).await?;

    // --- Publish side: create broadcast for our telemetry ---
    let mut broadcast = producer
        .create_broadcast(&drone_path)
        .expect("failed to create drone broadcast");
    let mut position_track = broadcast.create_track(Track::new(POSITION_TRACK));

    // --- Subscribe side: listen for commands addressed to us ---
    let cmd_broadcast = consumer
        .consume_broadcast(&control_path)
        .expect("failed to consume control broadcast");
    let mut cmd_track = cmd_broadcast.subscribe_track(&Track::new(COMMAND_TRACK));

    // Simulated drone state
    let mut lat = 37.7749;
    let mut lon = -122.4194;
    let mut alt = 100.0;
    let heading = 0.0;
    let speed = 5.0;

    // Target position (updated by commands)
    let mut target_lat = lat;
    let mut target_lon = lon;
    let mut target_alt = alt;

    println!("Drone {drone_id} is online.");

    let mut ticker = interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                // Simulate movement toward target
                let dlat: f64 = target_lat - lat;
                let dlon: f64 = target_lon - lon;
                let dalt: f64 = target_alt - alt;
                let step: f64 = 0.0001;
                if dlat.abs() > step { lat += step * dlat.signum(); }
                if dlon.abs() > step { lon += step * dlon.signum(); }
                if dalt.abs() > 0.5 { alt += 0.5 * dalt.signum(); }

                let pos = DronePosition {
                    drone_id: drone_id.clone(),
                    latitude: lat,
                    longitude: lon,
                    altitude_m: alt,
                    heading_deg: heading,
                    speed_mps: speed,
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };

                let mut buf = Vec::with_capacity(pos.encoded_len());
                pos.encode(&mut buf)?;
                position_track.write_frame(buf);

                println!(
                    "[TX] position: lat={:.6}, lon={:.6}, alt={:.1}m",
                    pos.latitude, pos.longitude, pos.altitude_m
                );
            }

            result = cmd_track.next_group() => {
                match result {
                    Ok(Some(mut group)) => {
                        while let Ok(Some(frame)) = group.read_frame().await {
                            let cmd = drone_proto::DroneCommand::decode(frame.as_ref())?;
                            println!(
                                "[RX] command: {:?} -> ({:.6}, {:.6}, {:.1}m)",
                                drone_proto::CommandType::try_from(cmd.command)
                                    .unwrap_or(drone_proto::CommandType::Hover),
                                cmd.target_lat,
                                cmd.target_lon,
                                cmd.target_alt_m,
                            );

                            match drone_proto::CommandType::try_from(cmd.command) {
                                Ok(drone_proto::CommandType::Goto) => {
                                    target_lat = cmd.target_lat;
                                    target_lon = cmd.target_lon;
                                    target_alt = cmd.target_alt_m;
                                }
                                Ok(drone_proto::CommandType::Hover) => {
                                    target_lat = lat;
                                    target_lon = lon;
                                    target_alt = alt;
                                }
                                Ok(drone_proto::CommandType::Land) => {
                                    target_alt = 0.0;
                                    target_lat = lat;
                                    target_lon = lon;
                                }
                                Ok(drone_proto::CommandType::ReturnHome) => {
                                    target_lat = 37.7749;
                                    target_lon = -122.4194;
                                    target_alt = 100.0;
                                }
                                Err(_) => {
                                    println!("[RX] unknown command type: {}", cmd.command);
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        println!("Command track closed");
                        break;
                    }
                    Err(e) => {
                        // Subscription errors are expected when no controller is
                        // publishing yet. Keep retrying.
                        println!("Command track error (will retry): {e}");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        cmd_track = cmd_broadcast.subscribe_track(&Track::new(COMMAND_TRACK));
                    }
                }
            }
        }
    }

    Ok(())
}
