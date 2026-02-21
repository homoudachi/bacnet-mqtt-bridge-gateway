# BACnet-MQTT Bridge Gateway

A robust, asynchronous Rust application that bridges the gap between Building Automation systems (BACnet/IP) and IoT platforms (MQTT). It automatically discovers BACnet devices on your local network, polls them for data, and pushes the readings to an MQTT broker. It is specifically designed with Home Assistant auto-discovery in mind!

## 🚀 Features

*   **Auto-Discovery:** Automatically broadcasts BACnet `Who-Is` requests and registers responding devices.
*   **Home Assistant Integration:** Immediately publishes MQTT discovery payloads for seamless integration into Home Assistant.
*   **Asynchronous Polling:** Uses `tokio` to concurrently poll discovered BACnet devices (e.g., Analog Input points) without blocking the main event loop.
*   **Robust Decoding:** Built on `bacnet-rs` to reliably parse NPDU and APDU network structures.

## 🏗️ Project Structure

The workspace contains two core binaries:

1.  **`bacnet-mqtt-gateway`**: The main bridge application.
2.  **`bacnet-test-responder`**: A simulated BACnet software device useful for end-to-end testing without physical hardware.

## ⚙️ Configuration

The gateway uses a `config.toml` file (generated if not present) to manage its settings:

```toml
[bacnet]
bind_addr = "0.0.0.0:47808"
device_id = 1234
vendor_name = "My Automation Co"
model_name = "BACnet-MQTT Bridge"

[mqtt]
broker_url = "mqtt://localhost:1883"
client_id = "bacnet_bridge_01"
discovery_prefix = "homeassistant"
```

## 🛠️ Usage

### Prerequisites
*   [Rust toolchain](https://rustup.rs/) (cargo, rustc)
*   An running MQTT broker (like Eclipse Mosquitto)

### Running the Gateway

```bash
cd bacnet-mqtt-gateway
cargo run
```

### Running the Test Responder (Development)

If you don't have a real BACnet device on your network, you can run the test responder in a separate terminal:

```bash
cd bacnet-test-responder
RUST_LOG=info cargo run
```

The test responder simulates a device with `AnalogInput 0` (Present Value). Once running, it starts a lightweight HTTP API on port `8124` so you can interact with it dynamically during your end-to-end tests:

*   **Change Simulated Value:**
    ```bash
    curl -X POST http://localhost:8124/value/75.2
    ```
    *(The next time the gateway polls the responder, it will read `75.2` and publish it to MQTT).*

*   **Force an `I-Am` Broadcast:**
    ```bash
    curl -X POST http://localhost:8124/iam
    ```
    *(Useful for triggering the gateway's discovery mechanism manually).*

## 📖 Architecture

For a deeper dive into the technical design, requirements, and internal module responsibilities, please see the [Functional Specification Document (FSD)](FSD.md).

## 🤝 Contributing

Contributions, issues, and feature requests are welcome! Feel free to check the [issues page](../../issues).

## 📝 License

This project is licensed under the MIT License.
