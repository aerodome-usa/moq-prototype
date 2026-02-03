use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use prost::Message;
use tonic::{Request, Response, Status, Streaming};

use crate::drone::DroneSessionMap;
use crate::drone_proto::drone_message::Payload;
use crate::drone_proto::drone_service_server::{DroneService, DroneServiceServer};
use crate::drone_proto::{CommandAck, DroneCommand, DroneMessage};
use crate::state_machine::telemetry::Position;
use crate::unit::UnitId;
use crate::unit_context::UnitContext;
use crate::unit_map::UnitMap;

pub async fn start_server(
    addr: SocketAddr,
    unit_map: Arc<UnitMap<UnitContext>>,
    session_map: Arc<DroneSessionMap>,
) -> anyhow::Result<()> {
    let service = DroneServiceImpl::new(unit_map, session_map);

    println!("[gRPC] Server starting on {addr}");

    tonic::transport::Server::builder()
        .add_service(DroneServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}

pub struct DroneServiceImpl {
    unit_map: Arc<UnitMap<UnitContext>>,
    session_map: Arc<DroneSessionMap>,
}

impl DroneServiceImpl {
    pub fn new(unit_map: Arc<UnitMap<UnitContext>>, session_map: Arc<DroneSessionMap>) -> Self {
        Self {
            unit_map,
            session_map,
        }
    }
}

#[tonic::async_trait]
impl DroneService for DroneServiceImpl {
    type DroneSessionStream =
        Pin<Box<dyn futures::Stream<Item = Result<DroneMessage, Status>> + Send>>;

    async fn drone_session(
        &self,
        request: Request<Streaming<DroneMessage>>,
    ) -> Result<Response<Self::DroneSessionStream>, Status> {
        let mut inbound = request.into_inner();

        // I need the first message to come in in order to get the drone ID.
        let first_msg = inbound
            .next()
            .await
            .ok_or_else(|| Status::invalid_argument("Empty stream"))?
            .map_err(|e| Status::internal(e.to_string()))?;

        let drone_id = match &first_msg.payload {
            Some(Payload::Position(pos)) => pos.drone_id.clone(),
            _ => return Err(Status::invalid_argument("First message must be position")),
        };

        let unit_id = UnitId::from(drone_id.as_str());

        println!("[gRPC] DroneSession started for {drone_id}");

        // Create or reuse unit context
        if self.unit_map.get_unit(&unit_id).is_err() {
            let context = UnitContext::new();
            self.unit_map
                .insert_unit(unit_id.clone(), context)
                .map_err(|e| Status::internal(e.to_string()))?;
            println!("[gRPC] Created UnitContext for {drone_id}");
        }

        match self.session_map.create_session(&unit_id) {
            Ok(session_id) => {
                println!("[gRPC] Session created for {drone_id}: {session_id}");
            }
            Err(e) => {
                return Err(Status::already_exists(e.to_string()));
            }
        }

        // Process that first telemetry message
        if let Some(Payload::Position(pos)) = first_msg.payload {
            self.process_telemetry(&unit_id, pos);
        }

        // Spawn task to process telemetry → StateMachine
        let unit_map_for_telemetry = Arc::clone(&self.unit_map);
        let telemetry_session_map = Arc::clone(&self.session_map);
        let unit_id_for_telemetry = unit_id.clone();
        let drone_id_for_task = drone_id.clone();

        tokio::spawn(async move {
            while let Some(msg_result) = inbound.next().await {
                match msg_result {
                    Ok(DroneMessage {
                        payload: Some(Payload::Position(pos)),
                    }) => {
                        let position = Position {
                            drone_id: pos.drone_id.clone(),
                            latitude: pos.latitude,
                            longitude: pos.longitude,
                            altitude_m: pos.altitude_m,
                            heading_deg: pos.heading_deg,
                            speed_mps: pos.speed_mps,
                            timestamp: pos.timestamp,
                        };

                        if let Ok(unit_ref) =
                            unit_map_for_telemetry.get_unit(&unit_id_for_telemetry)
                        {
                            let _ = unit_ref.view(|ctx| ctx.update_telemetry(position));
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        println!("[gRPC] Telemetry stream error for {drone_id_for_task}: {e}");
                        break;
                    }
                }
            }

            // Cleanup on disconnect
            println!("[gRPC] Telemetry stream closed for {drone_id_for_task}");
            let _ = telemetry_session_map.remove_session(&unit_id_for_telemetry);
        });

        let unit_map_for_commands = Arc::clone(&self.unit_map);
        let session_map_for_stream = Arc::clone(&self.session_map);
        let unit_id_for_stream = unit_id.clone();
        let drone_id_for_stream = drone_id.clone();

        let outbound = async_stream::stream! {
            loop {
                if !session_map_for_stream.has_active_session(&unit_id_for_stream) {
                    println!("[gRPC] Session ended, closing command stream for {drone_id_for_stream}");
                    break;
                }

                let maybe_cmd = unit_map_for_commands
                    .get_unit(&unit_id_for_stream)
                    .ok()
                    .and_then(|unit_ref| {
                        unit_ref.view(|ctx| ctx.poll_command()).ok().flatten()
                    });

                if let Some(cmd_bytes) = maybe_cmd {
                    match DroneCommand::decode(cmd_bytes.as_slice()) {
                        Ok(cmd) => {
                            println!("[gRPC] Sending command to {drone_id_for_stream}: {:?}", cmd.command);
                            yield Ok(DroneMessage {
                                payload: Some(Payload::Command(cmd)),
                            });
                        }
                        Err(e) => {
                            println!("[gRPC] Failed to decode command: {e}");
                        }
                    }
                }

                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };

        Ok(Response::new(Box::pin(outbound)))
    }

    async fn send_command(
        &self,
        request: Request<DroneCommand>,
    ) -> Result<Response<CommandAck>, Status> {
        let cmd = request.into_inner();
        let unit_id = UnitId::from(cmd.drone_id.as_str());

        if !self.session_map.has_active_session(&unit_id) {
            return Err(Status::not_found(format!(
                "Drone {} not connected",
                cmd.drone_id
            )));
        }

        let mut buf = Vec::with_capacity(cmd.encoded_len());
        cmd.encode(&mut buf)
            .map_err(|e| Status::internal(format!("Encode error: {e}")))?;

        let unit_ref = self
            .unit_map
            .get_unit(&unit_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        unit_ref
            .view(|ctx| ctx.enqueue_command(buf))
            .map_err(|e| Status::internal(e.to_string()))?;

        println!(
            "[gRPC] Command enqueued for {}: {:?}",
            cmd.drone_id, cmd.command
        );

        Ok(Response::new(CommandAck {
            accepted: true,
            message: String::new(),
        }))
    }
}

impl DroneServiceImpl {
    fn process_telemetry(&self, unit_id: &UnitId, pos: crate::drone_proto::DronePosition) {
        let position = Position {
            drone_id: pos.drone_id,
            latitude: pos.latitude,
            longitude: pos.longitude,
            altitude_m: pos.altitude_m,
            heading_deg: pos.heading_deg,
            speed_mps: pos.speed_mps,
            timestamp: pos.timestamp,
        };

        if let Ok(unit_ref) = self.unit_map.get_unit(unit_id) {
            let _ = unit_ref.view(|ctx| ctx.update_telemetry(position));
        }
    }
}
