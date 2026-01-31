use anyhow::Result;
use moq_lite::Track;
use moq_prototype::drone_proto::{self, CommandType, DroneCommand, DronePosition};
use moq_prototype::{
    connect_bidirectional, control_broadcast_path, drone_broadcast_path, COMMAND_TRACK,
    POSITION_TRACK,
};
use prost::Message;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("RELAY_URL").unwrap_or_else(|_| "https://localhost:4443".to_string());
    let drone_id = std::env::var("DRONE_ID").expect("DRONE_ID is required for the controller");

    let drone_path = drone_broadcast_path(&drone_id);
    let control_path = control_broadcast_path(&drone_id);

    println!("Controller connecting to relay at {url}");
    println!("  Subscribing to position: {drone_path}/{POSITION_TRACK}");
    println!("  Publishing commands to: {control_path}/{COMMAND_TRACK}");

    let (_session, producer, consumer) = connect_bidirectional(&url).await?;

    // --- Subscribe side: receive drone position telemetry ---
    let pos_broadcast = consumer
        .consume_broadcast(&drone_path)
        .expect("failed to consume drone broadcast");
    let mut pos_track = pos_broadcast.subscribe_track(&Track::new(POSITION_TRACK));

    // --- Publish side: send commands to this drone ---
    let mut cmd_broadcast = producer
        .create_broadcast(&control_path)
        .expect("failed to create control broadcast");
    let mut cmd_track = cmd_broadcast.create_track(Track::new(COMMAND_TRACK));

    println!();
    println!("Connected to drone {drone_id}. Commands:");
    println!("  goto <lat> <lon> <alt>   - Fly to position");
    println!("  hover                    - Hold current position");
    println!("  land                     - Land at current position");
    println!("  home                     - Return to home");
    println!();

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        tokio::select! {
            result = pos_track.next_group() => {
                match result {
                    Ok(Some(mut group)) => {
                        while let Ok(Some(frame)) = group.read_frame().await {
                            let pos = DronePosition::decode(frame.as_ref())?;
                            println!(
                                "[RX] drone={} lat={:.6} lon={:.6} alt={:.1}m hdg={:.0} spd={:.1}m/s",
                                pos.drone_id, pos.latitude, pos.longitude,
                                pos.altitude_m, pos.heading_deg, pos.speed_mps,
                            );
                        }
                    }
                    Ok(None) => {
                        println!("Position track closed");
                        break;
                    }
                    Err(e) => {
                        println!("Position track error: {e}");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        pos_track = pos_broadcast.subscribe_track(&Track::new(POSITION_TRACK));
                    }
                }
            }

            line = lines.next_line() => {
                let line = match line? {
                    Some(l) => l,
                    None => break, // EOF
                };
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }

                let cmd = match parts[0] {
                    "goto" if parts.len() == 4 => {
                        let lat: f64 = parts[1].parse()?;
                        let lon: f64 = parts[2].parse()?;
                        let alt: f64 = parts[3].parse()?;
                        DroneCommand {
                            drone_id: drone_id.clone(),
                            command: CommandType::Goto.into(),
                            target_lat: lat,
                            target_lon: lon,
                            target_alt_m: alt,
                            timestamp: now(),
                        }
                    }
                    "hover" => DroneCommand {
                        drone_id: drone_id.clone(),
                        command: CommandType::Hover.into(),
                        target_lat: 0.0,
                        target_lon: 0.0,
                        target_alt_m: 0.0,
                        timestamp: now(),
                    },
                    "land" => DroneCommand {
                        drone_id: drone_id.clone(),
                        command: CommandType::Land.into(),
                        target_lat: 0.0,
                        target_lon: 0.0,
                        target_alt_m: 0.0,
                        timestamp: now(),
                    },
                    "home" => DroneCommand {
                        drone_id: drone_id.clone(),
                        command: CommandType::ReturnHome.into(),
                        target_lat: 0.0,
                        target_lon: 0.0,
                        target_alt_m: 0.0,
                        timestamp: now(),
                    },
                    _ => {
                        println!("Unknown command. Try: goto <lat> <lon> <alt>, hover, land, home");
                        continue;
                    }
                };

                let mut buf = Vec::with_capacity(cmd.encoded_len());
                cmd.encode(&mut buf)?;
                cmd_track.write_frame(buf);
                println!("[TX] sent {:?} to drone {drone_id}", drone_proto::CommandType::try_from(cmd.command).unwrap());
            }
        }
    }

    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
