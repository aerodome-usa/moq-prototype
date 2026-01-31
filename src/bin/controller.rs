use anyhow::Result;
use moq_lite::{OriginProducer, Track, TrackProducer};
use moq_prototype::drone_proto::{self, CommandType, DroneCommand, DronePosition};
use moq_prototype::{connect_bidirectional, control_broadcast_path, COMMAND_TRACK, POSITION_TRACK};
use prost::Message;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Shared state: command tracks keyed by drone_id, created on demand.
type CommandTracks = Arc<Mutex<HashMap<String, TrackProducer>>>;

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("RELAY_URL").unwrap_or_else(|_| "https://localhost:4443".to_string());

    println!("Controller connecting to relay at {url}");

    let (_session, producer, consumer) = connect_bidirectional(&url).await?;

    // Wrap the producer so we can create command broadcasts from any task.
    let producer = Arc::new(producer);
    let cmd_tracks: CommandTracks = Arc::new(Mutex::new(HashMap::new()));

    // Filter to broadcasts under "drone/" — paths come back as just the drone_id.
    let mut drone_announcements = consumer
        .with_root("drone/")
        .expect("drone prefix not authorized");

    println!();
    println!("Waiting for drones to connect...");
    println!();
    println!("Commands (target a specific drone by ID):");
    println!("  list                              - Show connected drones");
    println!("  goto <drone_id> <lat> <lon> <alt> - Fly drone to position");
    println!("  hover <drone_id>                  - Hold current position");
    println!("  land <drone_id>                   - Land at current position");
    println!("  home <drone_id>                   - Return to home");
    println!();

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    // Track connected drone IDs for the `list` command.
    let connected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    loop {
        tokio::select! {
            // --- Discovery: watch for new drone broadcasts ---
            announcement = drone_announcements.announced() => {
                match announcement {
                    Some((path, Some(broadcast))) => {
                        let drone_id = path.to_string();
                        println!("[+] Drone discovered: {drone_id}");
                        connected.lock().unwrap().push(drone_id.clone());

                        // Spawn a task to read this drone's position telemetry.
                        tokio::spawn(async move {
                            let mut track = broadcast.subscribe_track(&Track::new(POSITION_TRACK));
                            loop {
                                match track.next_group().await {
                                    Ok(Some(mut group)) => {
                                        while let Ok(Some(frame)) = group.read_frame().await {
                                            match DronePosition::decode(frame.as_ref()) {
                                                Ok(pos) => {
                                                    println!(
                                                        "[RX {drone_id}] lat={:.6} lon={:.6} alt={:.1}m hdg={:.0} spd={:.1}m/s",
                                                        pos.latitude, pos.longitude,
                                                        pos.altitude_m, pos.heading_deg, pos.speed_mps,
                                                    );
                                                }
                                                Err(e) => println!("[RX {drone_id}] decode error: {e}"),
                                            }
                                        }
                                    }
                                    Ok(None) => {
                                        println!("[-] Drone {drone_id} position track closed");
                                        break;
                                    }
                                    Err(e) => {
                                        println!("[!] Drone {drone_id} position error: {e}");
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Some((path, None)) => {
                        let drone_id = path.to_string();
                        println!("[-] Drone departed: {drone_id}");
                        connected.lock().unwrap().retain(|id| id != &drone_id);
                        cmd_tracks.lock().unwrap().remove(&drone_id);
                    }
                    None => {
                        println!("Announcement stream closed");
                        break;
                    }
                }
            }

            // --- Stdin: process commands ---
            line = lines.next_line() => {
                let line = match line? {
                    Some(l) => l,
                    None => break,
                };
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }

                match parts[0] {
                    "list" => {
                        let drones = connected.lock().unwrap();
                        if drones.is_empty() {
                            println!("No drones connected.");
                        } else {
                            println!("Connected drones ({}):", drones.len());
                            for id in drones.iter() {
                                println!("  {id}");
                            }
                        }
                    }
                    "goto" if parts.len() == 5 => {
                        let drone_id = parts[1];
                        let lat: f64 = parts[2].parse()?;
                        let lon: f64 = parts[3].parse()?;
                        let alt: f64 = parts[4].parse()?;
                        send_command(
                            &producer,
                            &cmd_tracks,
                            drone_id,
                            DroneCommand {
                                drone_id: drone_id.to_string(),
                                command: CommandType::Goto.into(),
                                target_lat: lat,
                                target_lon: lon,
                                target_alt_m: alt,
                                timestamp: now(),
                            },
                        )?;
                    }
                    "hover" if parts.len() == 2 => {
                        let drone_id = parts[1];
                        send_command(
                            &producer,
                            &cmd_tracks,
                            drone_id,
                            DroneCommand {
                                drone_id: drone_id.to_string(),
                                command: CommandType::Hover.into(),
                                target_lat: 0.0,
                                target_lon: 0.0,
                                target_alt_m: 0.0,
                                timestamp: now(),
                            },
                        )?;
                    }
                    "land" if parts.len() == 2 => {
                        let drone_id = parts[1];
                        send_command(
                            &producer,
                            &cmd_tracks,
                            drone_id,
                            DroneCommand {
                                drone_id: drone_id.to_string(),
                                command: CommandType::Land.into(),
                                target_lat: 0.0,
                                target_lon: 0.0,
                                target_alt_m: 0.0,
                                timestamp: now(),
                            },
                        )?;
                    }
                    "home" if parts.len() == 2 => {
                        let drone_id = parts[1];
                        send_command(
                            &producer,
                            &cmd_tracks,
                            drone_id,
                            DroneCommand {
                                drone_id: drone_id.to_string(),
                                command: CommandType::ReturnHome.into(),
                                target_lat: 0.0,
                                target_lon: 0.0,
                                target_alt_m: 0.0,
                                timestamp: now(),
                            },
                        )?;
                    }
                    _ => {
                        println!("Usage:");
                        println!("  list");
                        println!("  goto <drone_id> <lat> <lon> <alt>");
                        println!("  hover <drone_id>");
                        println!("  land <drone_id>");
                        println!("  home <drone_id>");
                    }
                }
            }
        }
    }

    Ok(())
}

/// Get or create a command track for the given drone. The command broadcast
/// is created lazily the first time we send a command to a drone.
fn get_or_create_cmd_track<'a>(
    producer: &OriginProducer,
    tracks: &'a CommandTracks,
    drone_id: &str,
) -> &'a CommandTracks {
    let mut map = tracks.lock().unwrap();
    if !map.contains_key(drone_id) {
        let control_path = control_broadcast_path(drone_id);
        let mut broadcast = producer
            .create_broadcast(&control_path)
            .expect("failed to create control broadcast");
        let track = broadcast.create_track(Track::new(COMMAND_TRACK));
        map.insert(drone_id.to_string(), track);
        println!("[*] Created command channel for drone {drone_id}");
    }
    tracks
}

fn send_command(
    producer: &OriginProducer,
    tracks: &CommandTracks,
    drone_id: &str,
    cmd: DroneCommand,
) -> Result<()> {
    get_or_create_cmd_track(producer, tracks, drone_id);

    let mut map = tracks.lock().unwrap();
    let track = map.get_mut(drone_id).unwrap();

    let cmd_type = drone_proto::CommandType::try_from(cmd.command).unwrap();
    let mut buf = Vec::with_capacity(cmd.encoded_len());
    cmd.encode(&mut buf)?;
    track.write_frame(buf);
    println!("[TX] sent {cmd_type:?} to drone {drone_id}");
    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
