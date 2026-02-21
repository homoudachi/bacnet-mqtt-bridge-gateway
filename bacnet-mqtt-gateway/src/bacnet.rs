use crate::config::BacnetConfig;
use bacnet_rs::{
    datalink::bip::BacnetIpDataLink,
    datalink::{DataLink, DataLinkAddress},
    network::Npdu,
    object::Device,
    service::{UnconfirmedServiceChoice, WhoIsRequest, IAmRequest, ReadPropertyRequest, ReadPropertyResponse},
    app::Apdu,
};
use tokio::sync::mpsc;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use tracing::{info, trace};

#[derive(Debug, Clone)]
pub enum BacnetEvent {
    WhoIs(WhoIsRequest, SocketAddr),
    IAm(IAmRequest, SocketAddr),
    ReadProperty(ReadPropertyRequest, u8, SocketAddr),
    ReadPropertyAck(ReadPropertyResponse, u8, SocketAddr),
}

pub struct BacnetEngine {
    config: BacnetConfig,
    datalink: Arc<std::sync::Mutex<BacnetIpDataLink>>,
    device: Device,
    invoke_id: AtomicU8,
}

impl BacnetEngine {
    pub fn new(config: BacnetConfig) -> Result<Self, Box<dyn std::error::Error>> {
        info!("Initializing BACnet IP on {}", config.bind_addr);
        
        let datalink = BacnetIpDataLink::new(config.bind_addr)?;
        
        let mut device = Device::new(config.device_id, "BACnet-MQTT Gateway".to_string());
        device.vendor_name = config.vendor_name.clone();
        device.model_name = config.model_name.clone();

        Ok(Self {
            config,
            datalink: Arc::new(std::sync::Mutex::new(datalink)),
            device,
            invoke_id: AtomicU8::new(1),
        })
    }

    /// Broadcasts a Who-Is over the network to discover other devices
    pub fn discover(&self) -> Result<(), Box<dyn std::error::Error>> {
        let whois = WhoIsRequest::new();
        let mut whois_buffer = Vec::new();
        whois.encode(&mut whois_buffer)?;

        let apdu = Apdu::UnconfirmedRequest {
            service_choice: UnconfirmedServiceChoice::WhoIs,
            service_data: whois_buffer,
        };
        let apdu_bytes = apdu.encode();

        let mut npdu = Npdu::new();
        npdu.control.expecting_reply = false;
        npdu.control.priority = 0; 
        
        // Encode NPDU and concatenate
        let mut packet = npdu.encode();
        packet.extend_from_slice(&apdu_bytes);

        if let Ok(mut dl) = self.datalink.lock() {
            dl.send_broadcast_npdu(&packet)?;
            info!("Broadcasted Who-Is request");
        }
        Ok(())
    }

    /// Sends a ReadPropertyRequest to a specific device
    pub fn read_property(
        &self,
        target: SocketAddr,
        object_identifier: bacnet_rs::object::ObjectIdentifier,
        property_identifier: u32,
    ) -> Result<u8, Box<dyn std::error::Error>> {
        let req = ReadPropertyRequest::new(object_identifier, property_identifier);
        let mut service_data = Vec::new();
        req.encode(&mut service_data)?;

        // Simple invoke ID generator
        let invoke_id = self.invoke_id.fetch_add(1, Ordering::Relaxed);

        let apdu = Apdu::ConfirmedRequest {
            segmented: false,
            more_follows: false,
            segmented_response_accepted: true,
            max_segments: bacnet_rs::app::MaxSegments::Unspecified,
            max_response_size: bacnet_rs::app::MaxApduSize::Up1476,
            invoke_id,
            sequence_number: None,
            proposed_window_size: None,
            service_choice: bacnet_rs::service::ConfirmedServiceChoice::ReadProperty,
            service_data,
        };

        let mut npdu = Npdu::new();
        npdu.control.expecting_reply = true;
        npdu.control.priority = 0;
        
        let mut packet = npdu.encode();
        packet.extend_from_slice(&apdu.encode());

        if let Ok(mut dl) = self.datalink.lock() {
            dl.send_unicast_npdu(&packet, target)?;
            trace!("Sent ReadProperty to {} for {:?}", target, object_identifier);
        }
        
        Ok(invoke_id)
    }

    /// Spawns the background Tokio task that constantly receives UDP BACnet datagrams
    pub async fn start(&self) -> mpsc::Receiver<BacnetEvent> {
        let (tx, rx) = mpsc::channel(100);
        let dl = self.datalink.clone();
        
        tokio::task::spawn_blocking(move || {
            loop {
                if let Ok(mut dl_lock) = dl.lock() {
                    if let Ok((buf, src)) = dl_lock.receive_frame() {
                        if !buf.is_empty() {
                            trace!("Received {} bytes from {:?}", buf.len(), src);
                            if let Ok((npdu, consumed)) = Npdu::decode(&buf) {
                                if buf.len() > consumed && !npdu.is_network_message() {
                                    let apdu_bytes = &buf[consumed..];
                                    if let Ok(apdu) = Apdu::decode(apdu_bytes) {
                                        let source_addr = match src {
                                            DataLinkAddress::Ip(addr) => addr,
                                            _ => continue,
                                        };

                                        let event_opt = match apdu {
                                            Apdu::UnconfirmedRequest { service_choice, service_data } => {
                                                match service_choice {
                                                    UnconfirmedServiceChoice::WhoIs => {
                                                        WhoIsRequest::decode(&service_data).ok().map(|req| BacnetEvent::WhoIs(req, source_addr))
                                                    }
                                                    UnconfirmedServiceChoice::IAm => {
                                                        IAmRequest::decode(&service_data).ok().map(|req| BacnetEvent::IAm(req, source_addr))
                                                    }
                                                    _ => None,
                                                }
                                            }
                                            Apdu::ConfirmedRequest { service_choice, service_data, invoke_id, .. } => {
                                                match service_choice {
                                                    bacnet_rs::service::ConfirmedServiceChoice::ReadProperty => {
                                                        tracing::trace!("ReadPropertyRequest decode not implemented in bacnet-rs yet");
                                                        None
                                                    }
                                                    _ => None,
                                                }
                                            }
                                            Apdu::ComplexAck { service_choice, service_data, invoke_id, .. } => {
                                                if service_choice == bacnet_rs::service::ConfirmedServiceChoice::ReadProperty as u8 {
                                                    ReadPropertyResponse::decode(&service_data).ok().map(|ack| BacnetEvent::ReadPropertyAck(ack, invoke_id, source_addr))
                                                } else {
                                                    None
                                                }
                                            }
                                            _ => None,
                                        };

                                        if let Some(event) = event_opt {
                                            if tx.blocking_send(event).is_err() {
                                                break; // Receiver disconnected
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Small sleep to prevent busy lock loops in sync block
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        
        rx
    }
}
