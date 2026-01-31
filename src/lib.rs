use anyhow::Result;
use moq_lite::{Client, Origin, Session};
use url::Url;
use web_transport_quinn::ClientBuilder;

pub mod drone_proto {
    include!(concat!(env!("OUT_DIR"), "/drone.rs"));
}

/// Broadcast path for a drone's outbound telemetry.
/// Published by the drone, subscribed to by the controller.
pub fn drone_broadcast_path(drone_id: &str) -> String {
    format!("drone/{drone_id}")
}

/// Broadcast path for inbound commands to a drone.
/// Published by the controller, subscribed to by the drone.
pub fn control_broadcast_path(drone_id: &str) -> String {
    format!("control/{drone_id}")
}

pub const POSITION_TRACK: &str = "position";
pub const COMMAND_TRACK: &str = "commands";

/// Connect to the relay as a publisher + subscriber (bidirectional).
/// Returns the session handle and the origin producer/consumer pair.
pub async fn connect_bidirectional(
    relay_url: &str,
) -> Result<(Session, moq_lite::OriginProducer, moq_lite::OriginConsumer)> {
    let pub_origin = Origin::produce();
    let sub_origin = Origin::produce();

    let wt_client = ClientBuilder::new()
        .dangerous()
        .with_no_certificate_verification()?;
    let wt_session = wt_client.connect(relay_url.parse::<Url>()?).await?;

    let client = Client::new()
        .with_publish(pub_origin.consumer)
        .with_consume(sub_origin.producer);
    let session = client.connect(wt_session).await?;

    // pub_origin.producer  -> use to create broadcasts and write data
    // sub_origin.consumer  -> use to subscribe to broadcasts and read data
    Ok((session, pub_origin.producer, sub_origin.consumer))
}
