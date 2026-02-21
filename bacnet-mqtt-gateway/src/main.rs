mod bacnet;
mod config;
mod mqtt;

use axum::{routing::get, Router, response::Html};
use config::GatewayConfig;
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    info!("Starting BACnet-MQTT Gateway...");

    // Try to load configuration, or spawn default
    let cfg = GatewayConfig::default();

    // Start BACnet engine
    let bacnet = bacnet::BacnetEngine::new(cfg.bacnet.clone())?;
    
    // Broadcast discover on startup
    if let Err(e) = bacnet.discover() {
        tracing::error!("Failed to send initial Who-Is: {}", e);
    }

    // Start background receive loop
    let mut bacnet_rx = bacnet.start().await;

    // Start MQTT background publisher
    let mqtt = mqtt::MqttService::new(cfg.mqtt.clone()).await?;

    // Spawn a task to bridge BACnet events to MQTT
    let bridge_mqtt = mqtt.clone();
    tokio::spawn(async move {
        while let Some(event) = bacnet_rx.recv().await {
            match event {
                bacnet::BacnetEvent::IAm(iam, src) => {
                    tracing::info!("Discovered BACnet device {} at {}", iam.device_identifier.instance, src);
                    
                    let unique_id = format!("bacnet_{}", iam.device_identifier.instance);
                    let payload = mqtt::HaDiscoveryPayload {
                        name: format!("BACnet Device {}", iam.device_identifier.instance),
                        state_topic: format!("{}/sensor/{}/state", cfg.mqtt.discovery_prefix, unique_id),
                        command_topic: None,
                        unique_id: unique_id.clone(),
                        device: mqtt::HaDevice {
                            identifiers: vec![unique_id.clone()],
                            name: format!("BACnet Device {}", iam.device_identifier.instance),
                            manufacturer: format!("Vendor ID {}", iam.vendor_identifier),
                            model: "Generic BACnet Device".to_string(),
                        },
                    };
                    
                    bridge_mqtt.publish_discovery("sensor", &unique_id, &payload).await;
                    bridge_mqtt.publish_state(&payload.state_topic, "online").await;
                }
                bacnet::BacnetEvent::WhoIs(req, src) => {
                    tracing::debug!("Received Who-Is from {} for range {:?}", src, (req.device_instance_range_low_limit, req.device_instance_range_high_limit));
                }
                bacnet::BacnetEvent::ReadProperty(req, _, src) => {
                    tracing::debug!("Received ReadProperty from {} for {:?}", src, req.object_identifier);
                }
                bacnet::BacnetEvent::ReadPropertyAck(ack, _, src) => {
                    tracing::debug!("Received ReadPropertyAck from {} for {:?}", src, ack.object_identifier);
                }
            }
        }
    });

    // Build the configuration Web UI
    let app = Router::new().route("/", get(serve_ui));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8123));
    info!("Web UI listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn serve_ui() -> Html<&'static str> {
    Html("<html><body><h1>BACnet-MQTT Gateway</h1><p>Gateway configuration will be generated here.</p></body></html>")
}
