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

    /// Spawns the background Tokio task that constantly receives UDP BACnet datagrams
    pub async fn start(self) -> mpsc::Receiver<BacnetEvent> {
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
                                            // TODO: Confirmed requests and acknowledgements decoding
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
