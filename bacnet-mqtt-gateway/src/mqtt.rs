use crate::config::MqttConfig;
use rumqttc::{AsyncClient, MqttOptions, QoS};
use serde::Serialize;
use std::time::Duration;
use tracing::{error, info};

#[derive(Clone)]
pub struct MqttService {
    client: AsyncClient,
    config: MqttConfig,
}

#[derive(Serialize)]
pub struct HaDiscoveryPayload {
    pub name: String,
    pub state_topic: String,
    pub command_topic: Option<String>,
    pub unique_id: String,
    pub device: HaDevice,
}

#[derive(Serialize)]
pub struct HaDevice {
    pub identifiers: Vec<String>,
    pub name: String,
    pub manufacturer: String,
    pub model: String,
}

impl MqttService {
    pub async fn new(config: MqttConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let mut mqttoptions = MqttOptions::new(
            format!("bacnet-gateway-{}", std::process::id()),
            &config.broker_host,
            config.broker_port,
        );
        mqttoptions.set_keep_alive(Duration::from_secs(5));
        
        if let (Some(u), Some(p)) = (&config.username, &config.password) {
            mqttoptions.set_credentials(u, p);
        }

        let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

        // Spawn background task to keep the MQTT connection and receive events
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(event) => {
                        // TODO: Handle incoming command messages here and route them back to BACnet
                        tracing::trace!("MQTT Event: {:?}", event);
                    }
                    Err(e) => {
                        tracing::error!("MQTT Connection Error: {:?}", e);
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                }
            }
        });

        Ok(Self { client, config })
    }

    /// Publishes a Home Assistant Auto-Discovery payload for a sensor/binary_sensor
    pub async fn publish_discovery(&self, component: &str, unique_id: &str, payload: &HaDiscoveryPayload) {
        let topic = format!("{}/{}/{}/config", self.config.discovery_prefix, component, unique_id);
        
        if let Ok(json) = serde_json::to_string(payload) {
            if let Err(e) = self.client.publish(topic, QoS::AtLeastOnce, true, json).await {
                error!("Failed to publish discovery: {}", e);
            } else {
                info!("Published discovery for {}", unique_id);
            }
        }
    }

    /// Publishes a state update
    pub async fn publish_state(&self, topic: &str, value: &str) {
        if let Err(e) = self.client.publish(topic, QoS::AtLeastOnce, true, value).await {
            error!("Failed to publish state {}: {}", topic, e);
        }
    }
}
