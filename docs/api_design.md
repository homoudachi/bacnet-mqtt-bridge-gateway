# API & Architecture Design

This document details the internal Rust module design for the BACnet-MQTT Gateway.

## 1. Modules

### `bacnet-mqtt-gateway::bacnet`
* **Purpose:** Native async Rust logic for the BACnet protocol using `bacnet-rs`.
* **API:**
  * `BacnetEngine::new(config)`: Binds a Tokio `UdpSocket` for BACnet/IP.
  * `BacnetEngine::start()`: Spawns the background Tokio loop to receive raw UDP datagrams and parse them using `bacnet-rs`.
  * `BacnetEngine::discover()`: Builds and sends a `Who-Is` unconfirmed request.
  * `BacnetEngine::subscribe_cov(...)`: Sends a confirmed `SubscribeCOV` request.
  * `BacnetEngine::write_property(...)`: Sends a confirmed `WriteProperty` request.

### `bacnet-mqtt-gateway::mqtt`
* **Purpose:** Handles the connection to the MQTT Broker and formatting of payloads via `rumqttc`.
* **API:**
  * `MqttClient::new(config)`: Connects to the broker.
  * `MqttClient::publish_discovery(device, object)`: Formats the JSON payload specifically for Home Assistant's `homeassistant/sensor/.../config` topics.
  * `MqttClient::publish_state(device, object, value)`: Formats the JSON state payload for the `state` topics.
  * `MqttClient::subscribe_commands()`: Subscribes to the corresponding `/set` topics to receive control payloads from Home Assistant.

### `bacnet-mqtt-gateway::gateway`
* **Purpose:** The central coordinator. Connects the async channels between the BACnet engine and the MQTT client.
* **API:**
  * `Gateway::run()`: The main entry point loop.

## 2. Configuration Schema (`config.yaml`)

```yaml
gateway:
  bacnet_device_id: 1234
  bind_ip: "0.0.0.0"
  port: 47808

mqtt:
  broker: "192.168.1.100"
  port: 1883
  username: "mqtt_user"
  password: "mqtt_password"
  discovery_prefix: "homeassistant"
  topic_prefix: "bacnet"
```
