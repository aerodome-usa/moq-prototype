use anyhow::Result;
use moq_lite::{Client, Origin, Track};
use prost::Message;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::interval;
use tracing::{debug, info};
use url::Url;
use web_transport_quinn::ClientBuilder;

pub mod telemetry {
    include!(concat!(env!("OUT_DIR"), "/telemetry.rs"));
}

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("RELAY_URL").unwrap_or_else(|_| "https://localhost:4443".to_string());
    let broadcast_path = std::env::var("BROADCAST_PATH").unwrap_or_else(|_| "sensors".to_string());
    let track_name = std::env::var("TRACK_NAME").unwrap_or_else(|_| "temperature".to_string());

    info!(
        relay = %url,
        broadcast = %broadcast_path,
        track = %track_name,
        "Connecting to relay"
    );

    let origin = Origin::produce();

    let wt_client = ClientBuilder::new()
        .dangerous()
        .with_no_certificate_verification()?;
    let wt_session = wt_client.connect(url.parse::<Url>()?).await?;

    let client = Client::new().with_publish(origin.consumer);
    let _moq_session = client.connect(wt_session).await?;

    let mut broadcast = origin
        .producer
        .create_broadcast(&broadcast_path)
        .expect("failed to create broadcast");
    let mut track = broadcast.create_track(Track::new(&track_name));

    info!(
        broadcast = %broadcast_path,
        track = %track_name,
        "Publishing started"
    );

    let mut ticker = interval(Duration::from_secs(1));
    let mut sequence = 0u64;

    loop {
        ticker.tick().await;

        let data = telemetry::SensorData {
            sensor_id: "thermostat-01".to_string(),
            temperature: 20.0 + (sequence as f64 % 10.0),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        let mut payload = Vec::with_capacity(data.encoded_len());
        data.encode(&mut payload)?;

        // Write a frame (creates a new group automatically)
        track.write_frame(payload);

        debug!(
            sequence = sequence,
            sensor_id = %data.sensor_id,
            temperature = data.temperature,
            "Published frame"
        );

        sequence += 1;
    }
}
