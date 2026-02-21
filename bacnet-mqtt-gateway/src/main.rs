mod bacnet;
mod config;
mod mqtt;

use axum::{routing::get, Router, response::Html};
use config::GatewayConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
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
    let bacnet = Arc::new(bacnet::BacnetEngine::new(cfg.bacnet.clone())?);
    
    // Broadcast discover on startup
    if let Err(e) = bacnet.discover() {
        tracing::error!("Failed to send initial Who-Is: {}", e);
    }

    // Start background receive loop
    let mut bacnet_rx = bacnet.start().await;

    // Start MQTT background publisher
    let mqtt = mqtt::MqttService::new(cfg.mqtt.clone()).await?;

    // Device registry
    let discovered_devices = Arc::new(RwLock::new(HashMap::<u32, SocketAddr>::new()));

    // Spawn a task to bridge BACnet events to MQTT
    let bridge_mqtt = mqtt.clone();
    let bridge_devices = discovered_devices.clone();
    let mqtt_prefix = cfg.mqtt.discovery_prefix.clone();
    tokio::spawn(async move {
        while let Some(event) = bacnet_rx.recv().await {
            match event {
                bacnet::BacnetEvent::IAm(iam, src) => {
                    tracing::info!("Discovered BACnet device {} at {}", iam.device_identifier.instance, src);
                    bridge_devices.write().await.insert(iam.device_identifier.instance, src);
                    
                    let unique_id = format!("bacnet_{}", iam.device_identifier.instance);
                    let payload = mqtt::HaDiscoveryPayload {
                        name: format!("BACnet Device {}", iam.device_identifier.instance),
                        state_topic: format!("{}/sensor/{}/state", mqtt_prefix, unique_id),
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
                    // Decode property value if it is PresentValue (85)
                    if ack.property_identifier == 85 {
                        if let Ok((val, _)) = bacnet_rs::encoding::decode_real(&ack.property_value) {
                            tracing::info!("Device {} AI {} Value: {}", ack.object_identifier.instance, ack.object_identifier.instance, val);
                            
                            let unique_id = format!("bacnet_{}", src.ip()); // Or use device instance if we mapped it, but src is easiest here
                            let state_topic = format!("{}/sensor/{}/state", mqtt_prefix, unique_id);
                            
                            // To actually map the IP to device instance, we should use bridge_devices
                            let mut device_id_opt = None;
                            for (id, addr) in bridge_devices.read().await.iter() {
                                if *addr == src {
                                    device_id_opt = Some(*id);
                                    break;
                                }
                            }

                            if let Some(dev_id) = device_id_opt {
                                let unique_id = format!("bacnet_{}", dev_id);
                                let state_topic = format!("{}/sensor/{}/state", mqtt_prefix, unique_id);
                                bridge_mqtt.publish_state(&state_topic, &val.to_string()).await;
                            }
                        } else {
                            tracing::debug!("Property 85 Value (raw): {:?}", ack.property_value);
                        }
                    }
                }
            }
        }
    });

    // Start Polling task
    let poll_bacnet = bacnet.clone();
    let poll_devices = discovered_devices.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            let devices = poll_devices.read().await.clone();
            for (device_id, addr) in devices {
                tracing::debug!("Polling device {} at {}", device_id, addr);
                // Analog Input 0 (0 << 22 | 0) => instance 0
                let ai_0 = bacnet_rs::object::ObjectIdentifier::new(bacnet_rs::object::ObjectType::AnalogInput, 0);
                if let Err(e) = poll_bacnet.read_property(addr, ai_0, 85) {
                    tracing::error!("Failed to poll {} AI 0: {}", device_id, e);
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
