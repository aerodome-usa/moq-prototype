pub mod drone;
pub mod grpc;
pub mod rpcmoq_lite;
pub mod state_machine;
pub mod unit;
pub mod unit_context;
pub mod unit_map;

use anyhow::Result;
use moq_lite::{Client, Origin, Session};
use url::Url;
use web_transport_quinn::ClientBuilder;

pub mod drone_proto {
    include!(concat!(env!("OUT_DIR"), "/drone.rs"));
}

pub const PRIMARY_TRACK: &str = "primary";

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

    Ok((session, pub_origin.producer, sub_origin.consumer))
}
