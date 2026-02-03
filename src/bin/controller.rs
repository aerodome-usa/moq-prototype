use anyhow::Result;
use async_stream::stream;
use moq_lite::{BroadcastConsumer, OriginProducer, Track, TrackProducer};
use moq_prototype::drone::DroneSessionMap;
use moq_prototype::drone_proto::drone_message::Payload;
use moq_prototype::drone_proto::{CommandType, DroneCommand, DroneMessage, DronePosition};
use moq_prototype::grpc::{self, DroneServiceClient};
use moq_prototype::unit::UnitId;
use moq_prototype::unit_context::UnitContext;
use moq_prototype::unit_map::UnitMap;
use moq_prototype::{COMMAND_TRACK, POSITION_TRACK, connect_bidirectional, control_broadcast_path};
use prost::Message;
use rand::Rng;
use std::sync::Arc;
use std::time::Duration;

const GRPC_ADDR: &str = "[::1]:50051";

/// Interval between random commands sent to each drone.
const RANDOM_COMMAND_INTERVAL: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("RELAY_URL").unwrap_or_else(|_| "https://localhost:4443".to_string());

    let unit_map: Arc<UnitMap<UnitContext>> = Arc::new(UnitMap::new());
    let session_map: Arc<DroneSessionMap> = Arc::new(DroneSessionMap::new());

    let grpc_addr = GRPC_ADDR.parse()?;
    let server_unit_map = Arc::clone(&unit_map);
    let server_session_map = Arc::clone(&session_map);
    tokio::spawn(async move {
        if let Err(e) = grpc::start_server(grpc_addr, server_unit_map, server_session_map).await {
            eprintln!("[!] gRPC server error: {e}");
        }
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    println!("Controller connecting to relay at {url}");

    let (_session, producer, consumer) = connect_bidirectional(&url).await?;
    let producer = Arc::new(producer);

    let mut drone_announcements = consumer
        .with_root("drone/")
        .expect("drone prefix not authorized");

    println!("Waiting for drones to connect...");

    loop {
        match drone_announcements.announced().await {
            Some((path, Some(broadcast))) => {
                let drone_id = path.to_string();
                println!("[+] Drone discovered: {drone_id}");

                spawn_drone_bridge(drone_id.clone(), broadcast, Arc::clone(&producer));
                spawn_random_command_task(drone_id, Arc::clone(&session_map));
            }

            // Drone disconnects
            Some((path, None)) => {
                let drone_id = path.to_string();
                println!("[-] Drone departed: {drone_id}");
                // stuff cleans up when streams start closing
            }

            None => {
                println!("Announcement stream closed");
                break;
            }
        }
    }

    Ok(())
}

fn spawn_drone_bridge(
    drone_id: String,
    broadcast: BroadcastConsumer,
    producer: Arc<OriginProducer>,
) {
    tokio::spawn(async move {
        // FIXME: how tf do I report errors back to the drone
        if let Err(e) = run_drone_bridge(drone_id.clone(), broadcast, producer).await {
            eprintln!("[!] Bridge error for {drone_id}: {e}");
        }
    });
}

async fn run_drone_bridge(
    drone_id: String,
    broadcast: BroadcastConsumer,
    producer: Arc<OriginProducer>,
) -> Result<()> {
    // create the broadcasts so the bidirectoinal comms are open.
    let mut client = DroneServiceClient::connect(format!("http://{GRPC_ADDR}")).await?;
    let mut track = broadcast.subscribe_track(&Track::new(POSITION_TRACK));

    let control_path = control_broadcast_path(&drone_id);
    let mut cmd_broadcast = producer
        .create_broadcast(&control_path)
        .ok_or_else(|| anyhow::anyhow!("Failed to create control broadcast"))?;
    let mut cmd_track: TrackProducer = cmd_broadcast.create_track(Track::new(COMMAND_TRACK));

    let drone_id_clone = drone_id.clone();

    let stream = stream! {
        loop {
            match track.next_group().await {
                Ok(Some(mut group)) => {
                    while let Ok(Some(frame)) = group.read_frame().await {
                        if let Ok(pos) = DronePosition::decode(frame.as_ref()) {
                            println!(
                                "[RX {drone_id_clone}] lat={:.6} lon={:.6} alt={:.1}m",
                                pos.latitude, pos.longitude, pos.altitude_m,
                            );
                            let msg = DroneMessage {
                                payload: Some(Payload::Position(pos)),
                            };
                            yield msg;
                        }
                    }
                }
                Ok(None) => {
                    println!("[-] Telemetry stream closed for {drone_id_clone}");
                    break;
                }
                Err(e) => {
                    println!("[!] Telemetry stream error for {drone_id_clone}: {e}");
                    break;
                }
            }
        }
    };
    let response = client.drone_session(stream).await?;
    let mut command_stream = response.into_inner();

    println!("[*] Bridge established for {drone_id}");

    while let Some(msg) = command_stream.message().await? {
        if let Some(Payload::Command(cmd)) = msg.payload {
            println!("[TX {drone_id}] command: {:?}", cmd.command);
            let mut buf = Vec::with_capacity(cmd.encoded_len());
            cmd.encode(&mut buf)?;
            cmd_track.write_frame(buf);
        }
    }

    println!("[*] Bridge closed for {drone_id}");

    Ok(())
}

/// Spawns a task that sends random commands to a drone via gRPC unary calls.
/// The task runs until the drone disconnects.
fn spawn_random_command_task(drone_id: String, session_map: Arc<DroneSessionMap>) {
    tokio::spawn(async move {
        if let Err(e) = run_random_command_task(drone_id.clone(), session_map).await {
            eprintln!("[!] Random command task error for {drone_id}: {e}");
        }
    });
}

async fn run_random_command_task(
    drone_id: String,
    session_map: Arc<DroneSessionMap>,
) -> Result<()> {
    // let shit connect first
    tokio::time::sleep(Duration::from_millis(500)).await;

    let unit_id = UnitId::from(drone_id.as_str());

    if !session_map.has_active_session(&unit_id) {
        println!("[CMD {drone_id}] Drone not connected, skipping random command task");
        return Ok(());
    }

    let mut client = DroneServiceClient::connect(format!("http://{GRPC_ADDR}")).await?;

    println!("[CMD {drone_id}] Random command task started");

    loop {
        if !session_map.has_active_session(&unit_id) {
            println!("[CMD {drone_id}] Drone disconnected, stopping random command task");
            break;
        }

        let command = generate_random_command(&drone_id);
        let command_type = command.command();

        match client.send_command(command).await {
            Ok(response) => {
                let ack = response.into_inner();
                if ack.accepted {
                    println!("[CMD {drone_id}] Sent random command: {command_type:?}");
                } else {
                    println!(
                        "[CMD {drone_id}] Command rejected: {:?} - {}",
                        command_type, ack.message
                    );
                }
            }
            Err(e) => {
                println!("[CMD {drone_id}] Failed to send command: {e}");
                if !session_map.has_active_session(&unit_id) {
                    println!("[CMD {drone_id}] Drone disconnected, stopping random command task");
                    break;
                }
            }
        }

        tokio::time::sleep(RANDOM_COMMAND_INTERVAL).await;
    }

    println!("[CMD {drone_id}] Random command task stopped");
    Ok(())
}

fn generate_random_command(drone_id: &str) -> DroneCommand {
    let mut rng = rand::rng();

    let command = match rng.random_range(0..4) {
        0 => CommandType::Goto,
        1 => CommandType::Hover,
        2 => CommandType::Land,
        _ => CommandType::ReturnHome,
    };

    let (target_lat, target_lon, target_alt_m) = if command == CommandType::Goto {
        (
            rng.random_range(-90.0..90.0),
            rng.random_range(-180.0..180.0),
            rng.random_range(10.0..500.0),
        )
    } else {
        (0.0, 0.0, 0.0)
    };

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    DroneCommand {
        drone_id: drone_id.to_string(),
        command: command.into(),
        target_lat,
        target_lon,
        target_alt_m,
        timestamp,
    }
}
