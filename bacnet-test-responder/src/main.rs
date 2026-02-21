use bacnet_rs::{
    datalink::bip::BacnetIpDataLink,
    datalink::{DataLink, DataLinkAddress},
    network::Npdu,
    object::Device,
    service::{UnconfirmedServiceChoice, WhoIsRequest, IAmRequest, ReadPropertyRequest, ReadPropertyResponse},
    app::Apdu,
};
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

struct ResponderState {
    device: Device,
    datalink: Arc<std::sync::Mutex<BacnetIpDataLink>>,
    current_value: f32,
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
        current_value: 24.5,
    }));

    // Start BACnet receiver loop
    let state_clone_for_rx = state.clone();
    let dl_clone = state.lock().await.datalink.clone();
    tokio::task::spawn_blocking(move || {
        loop {
            // Re-acquire device config and value dynamically every iteration from the lock
            // We use try_lock so we don't block the network loop if the HTTP server is holding it
            let (device_clone, current_value) = {
                if let Ok(st) = state_clone_for_rx.try_lock() {
                    (st.device.clone(), st.current_value)
                } else {
                    // Fallback
                    let mut fallback_dev = Device::new(99999, "Test Responder".into());
                    fallback_dev.vendor_name = "Automated Test Vendor".into();
                    (fallback_dev, 24.5)
                }
            };
            if let Ok(mut dl_lock) = dl_clone.lock() {
                if let Ok((buf, src)) = dl_lock.receive_frame() {
                    if !buf.is_empty() {
                        tracing::trace!("Responder received {} bytes from {:?}", buf.len(), src);
                        
                        let source_addr = match src {
                            DataLinkAddress::Ip(addr) => addr,
                            _ => continue,
                        };

                        if let Ok((npdu, consumed)) = Npdu::decode(&buf) {
                            if buf.len() > consumed && !npdu.is_network_message() {
                                let apdu_bytes = &buf[consumed..];
                                if let Ok(apdu) = Apdu::decode(apdu_bytes) {
                                    match apdu {
                                        Apdu::UnconfirmedRequest { service_choice, service_data } => {
                                            if service_choice == UnconfirmedServiceChoice::WhoIs {
                                                if let Ok(_) = WhoIsRequest::decode(&service_data) {
                                                    tracing::info!("Received Who-Is, sending I-Am");
                                                    // Construct I-Am
                                                    let iam = IAmRequest {
                                                        device_identifier: device_clone.identifier,
                                                        max_apdu_length_accepted: device_clone.max_apdu_length_accepted as u32,
                                                        segmentation_supported: device_clone.segmentation_supported as u32,
                                                        vendor_identifier: device_clone.vendor_identifier as u32,
                                                    };
                                                    let mut iam_buf = Vec::new();
                                                    iam.encode(&mut iam_buf).unwrap();

                                                    let iam_apdu = Apdu::UnconfirmedRequest {
                                                        service_choice: UnconfirmedServiceChoice::IAm,
                                                        service_data: iam_buf,
                                                    };
                                                    
                                                    let mut reply_npdu = Npdu::new();
                                                    reply_npdu.control.expecting_reply = false;
                                                    let mut packet = reply_npdu.encode();
                                                    packet.extend_from_slice(&iam_apdu.encode());
                                                    
                                                    // Broadcast I-Am globally
                                                    let _ = dl_lock.send_broadcast_npdu(&packet);
                                                }
                                            }
                                        },
                                        Apdu::ConfirmedRequest { service_choice, service_data, invoke_id, .. } => {
                                            if service_choice == bacnet_rs::service::ConfirmedServiceChoice::ReadProperty {
                                                // Minimal Read Property implementation
                                                tracing::info!("Received ReadPropertyRequest from {}", source_addr);
                                                
                                                // Hardcoded temperature response for AI 0 Property 85 (PresentValue)
                                                // Extract object/property manually or use hardcoded if not supported
                                                // (Since decode is not fully there for ReadPropertyRequest)
                                                
                                                
                                                // 1. ObjectIdentifier (Context tag 0)
                                                let mut ack_buf = Vec::new();
                                                ack_buf.extend_from_slice(&bacnet_rs::encoding::encode_context_object_id(
                                                    bacnet_rs::object::ObjectType::AnalogInput as u16, 0, 0
                                                ).unwrap());
                                                
                                                // 2. PropertyIdentifier (Context tag 1)
                                                ack_buf.extend_from_slice(&bacnet_rs::encoding::encode_context_enumerated(85, 1).unwrap());

                                                // 3. PropertyValue (Context tag 3, Opening Tag=6, Closing Tag=7)
                                                ack_buf.push(0x08 | (3 << 4) | 6); // Opening Tag 3
                                                bacnet_rs::encoding::encode_real(&mut ack_buf, current_value).unwrap(); 
                                                ack_buf.push(0x08 | (3 << 4) | 7); // Closing Tag 3
                                                
                                                let ack_apdu = Apdu::ComplexAck {
                                                    invoke_id,
                                                    service_choice: bacnet_rs::service::ConfirmedServiceChoice::ReadProperty as u8,
                                                    service_data: ack_buf,
                                                    segmented: false,
                                                    more_follows: false,
                                                    sequence_number: None,
                                                    proposed_window_size: None,
                                                };
                                                
                                                let mut reply_npdu = Npdu::new();
                                                reply_npdu.control.expecting_reply = false;
                                                let mut packet = reply_npdu.encode();
                                                packet.extend_from_slice(&ack_apdu.encode());
                                                
                                                let _ = dl_lock.send_unicast_npdu(&packet, source_addr);
                                            }
                                        },
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });

    // Start HTTP control server on port 8124
    let state_for_http = state.clone();
    let app = Router::new()
        .route("/status", get(|| async { "Responder Running!" }))
        .route("/value/:val", axum::routing::post({
            let st = state_for_http.clone();
            move |axum::extract::Path(val): axum::extract::Path<f32>| async move {
                st.lock().await.current_value = val;
                format!("Value updated to {}", val)
            }
        }))
        .route("/iam", axum::routing::post({
            let st = state_for_http.clone();
            move || async move {
                let st_lock = st.lock().await;
                if let Ok(mut dl_lock) = st_lock.datalink.lock() {
                    let iam = IAmRequest {
                        device_identifier: st_lock.device.identifier,
                        max_apdu_length_accepted: st_lock.device.max_apdu_length_accepted as u32,
                        segmentation_supported: st_lock.device.segmentation_supported as u32,
                        vendor_identifier: st_lock.device.vendor_identifier as u32,
                    };
                    let mut iam_buf = Vec::new();
                    iam.encode(&mut iam_buf).unwrap();

                    let iam_apdu = Apdu::UnconfirmedRequest {
                        service_choice: UnconfirmedServiceChoice::IAm,
                        service_data: iam_buf,
                    };
                    
                    let mut reply_npdu = Npdu::new();
                    reply_npdu.control.expecting_reply = false;
                    let mut packet = reply_npdu.encode();
                    packet.extend_from_slice(&iam_apdu.encode());
                    
                    let _ = dl_lock.send_broadcast_npdu(&packet);
                    "I-Am Broadcasted"
                } else {
                    "Failed to acquire Datalink lock"
                }
            }
        }));
    
    let http_addr: SocketAddr = "0.0.0.0:8124".parse()?;
    info!("HTTP Control Server listening on {}", http_addr);
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
