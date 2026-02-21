use serde::{Deserialize, Serialize};
use std::fs;
use std::net::SocketAddr;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GatewayConfig {
    pub bacnet: BacnetConfig,
    pub mqtt: MqttConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BacnetConfig {
    pub device_id: u32,
    pub bind_addr: SocketAddr,
    pub vendor_name: String,
    pub model_name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MqttConfig {
    pub broker_host: String,
    pub broker_port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub discovery_prefix: String,
    pub base_topic: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bacnet: BacnetConfig {
                device_id: 12345,
                bind_addr: "0.0.0.0:47808".parse().unwrap(),
                vendor_name: "Rust BACnet Gateway".to_string(),
                model_name: "MQTT Bridge V1".to_string(),
            },
            mqtt: MqttConfig {
                broker_host: "127.0.0.1".to_string(),
                broker_port: 1883,
                username: None,
                password: None,
                discovery_prefix: "homeassistant".to_string(),
                base_topic: "bacnet".to_string(),
            },
        }
    }
}

impl GatewayConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&contents)?;
        Ok(config)
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let yaml = serde_yaml::to_string(self)?;
        fs::write(path, yaml)?;
        Ok(())
    }
}
