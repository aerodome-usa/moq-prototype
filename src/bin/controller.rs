use anyhow::Result;
use moq_lite::{BroadcastProducer, Track, TrackProducer};
use moq_prototype::drone_proto::{self, CommandType, DroneCommand, DronePosition};
use moq_prototype::{COMMAND_TRACK, POSITION_TRACK, connect_bidirectional, control_broadcast_path};
use prost::Message;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Store both BroadcastProducer (to keep broadcast alive) and TrackProducer (to send commands)
type CommandTracks = Arc<Mutex<HashMap<String, (BroadcastProducer, TrackProducer)>>>;

const COMMANDS: [CommandType; 4] = [
    CommandType::Goto,
    CommandType::Hover,
    CommandType::Land,
    CommandType::ReturnHome,
];

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("RELAY_URL").unwrap_or_else(|_| "https://localhost:4443".to_string());

    println!("Controller connecting to relay at {url}");

    let (_session, producer, consumer) = connect_bidirectional(&url).await?;

    let producer = Arc::new(producer);
    let cmd_tracks: CommandTracks = Arc::new(Mutex::new(HashMap::new()));

    let mut drone_announcements = consumer
        .with_root("drone/")
        .expect("drone prefix not authorized");

    println!("Waiting for drones to connect...");

    loop {
        match drone_announcements.announced().await {
            Some((path, Some(broadcast))) => {
                let drone_id = path.to_string();
                println!("[+] Drone discovered: {drone_id}");

                let mut map = cmd_tracks.lock().unwrap();
                {
                    if !map.contains_key(&drone_id) {
                        let control_path = control_broadcast_path(&drone_id);
                        let mut cmd_broadcast = producer
                            .create_broadcast(&control_path)
                            .expect("failed to create control broadcast");
                        let track = cmd_broadcast.create_track(Track::new(COMMAND_TRACK));
                        // Store both broadcast and track - broadcast must stay alive for subscriptions to work
                        map.insert(drone_id.clone(), (cmd_broadcast, track));
                        println!("[*] Created command channel for drone {drone_id}");
                    } else {
                        // a second announce means the broadcast is closed
                        map.remove(&drone_id);
                    }
                }

                // Spawn telemetry reader for this drone.
                let reader_id = drone_id.clone();
                tokio::spawn(async move {
                    let mut track = broadcast.subscribe_track(&Track::new(POSITION_TRACK));
                    loop {
                        match track.next_group().await {
                            Ok(Some(mut group)) => {
                                while let Ok(Some(frame)) = group.read_frame().await {
                                    match DronePosition::decode(frame.as_ref()) {
                                        Ok(pos) => {
                                            println!(
                                                "[RX {reader_id}] lat={:.6} lon={:.6} alt={:.1}m",
                                                pos.latitude, pos.longitude, pos.altitude_m,
                                            );
                                        }
                                        Err(e) => {
                                            println!("[RX {reader_id}] decode error: {e}");
                                        }
                                    }
                                }
                            }
                            Ok(None) => {
                                println!("[-] Drone {reader_id} position track closed");
                                break;
                            }
                            Err(e) => {
                                println!("[!] Drone {reader_id} position error: {e}");
                                break;
                            }
                        }
                    }
                });

                // Spawn random command sender for this drone.
                let sender_tracks = Arc::clone(&cmd_tracks);
                tokio::spawn(async move {
                    let mut rng = StdRng::from_os_rng();
                    loop {
                        tokio::time::sleep(Duration::from_secs(rng.random_range(2..6))).await;

                        let cmd_type = COMMANDS[rng.random_range(0..COMMANDS.len())];
                        let cmd = DroneCommand {
                            drone_id: drone_id.clone(),
                            command: cmd_type.into(),
                            target_lat: rng.random_range(37.0..38.0),
                            target_lon: rng.random_range(-123.0..-122.0),
                            target_alt_m: rng.random_range(50.0..500.0),
                            timestamp: SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                        };

                        if let Err(e) = send_command(&sender_tracks, &drone_id, cmd) {
                            println!("[!] Failed to send command to {drone_id}: {e}");
                            break;
                        }
                    }
                });
            }
            Some((path, None)) => {
                let drone_id = path.to_string();
                println!("[-] Drone departed: {drone_id}");
                cmd_tracks.lock().unwrap().remove(&drone_id);
            }
            None => {
                println!("Announcement stream closed");
                break;
            }
        }
    }

    Ok(())
}

fn send_command(tracks: &CommandTracks, drone_id: &str, cmd: DroneCommand) -> Result<()> {
    let mut map = tracks.lock().unwrap();
    let (_broadcast, track) = map
        .get_mut(drone_id)
        .expect("command track should exist for discovered drone");

    let cmd_type = drone_proto::CommandType::try_from(cmd.command).unwrap();
    let mut buf = Vec::with_capacity(cmd.encoded_len());
    cmd.encode(&mut buf)?;
    track.write_frame(buf);
    println!("[TX] sent {cmd_type:?} to drone {drone_id}");
    Ok(())
}
