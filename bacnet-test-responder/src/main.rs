use bacnet_rs::{
    datalink::bip::BacnetIpDataLink,
    datalink::DataLink,
    object::Device,
};
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

struct ResponderState {
    device: Device,
    datalink: Arc<std::sync::Mutex<BacnetIpDataLink>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    info!("Starting BACnet Test Responder...");

    let bind_addr: SocketAddr = "0.0.0.0:47809".parse()?; // Use a different port than the gateway
    let datalink = BacnetIpDataLink::new(bind_addr)?;
    
    let mut device = Device::new(99999, "Test Responder".to_string());
    device.vendor_name = "Automated Test Vendor".to_string();

    let state = Arc::new(Mutex::new(ResponderState {
        device,
        datalink: Arc::new(std::sync::Mutex::new(datalink)),
    }));

    // Start BACnet receiver loop
    let dl_clone = state.lock().await.datalink.clone();
    tokio::task::spawn_blocking(move || {
        loop {
            if let Ok(mut dl_lock) = dl_clone.lock() {
                if let Ok((buf, src)) = dl_lock.receive_frame() {
                    if !buf.is_empty() {
                        tracing::trace!("Responder received {} bytes from {:?}", buf.len(), src);
                        // TODO: Handle Who-Is, ReadProperty
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });

    // Start HTTP control server on port 8124
    let app = Router::new().route("/status", get(|| async { "Responder Running!" }));
    
    let http_addr: SocketAddr = "0.0.0.0:8124".parse()?;
    info!("HTTP Control Server listening on {}", http_addr);
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
