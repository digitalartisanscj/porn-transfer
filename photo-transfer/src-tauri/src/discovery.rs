use crate::DiscoveredService;
use mdns_sd::{ServiceDaemon, ServiceEvent};

const SERVICE_TYPE: &str = "_phototransfer._tcp.local.";

pub struct ServiceDiscovery {
    daemon: ServiceDaemon,
}

impl ServiceDiscovery {
    pub fn new<F>(mut on_service_found: F) -> Self
    where
        F: FnMut(DiscoveredService) + Send + 'static,
    {
        println!("mDNS Discovery: Starting daemon...");
        let daemon = ServiceDaemon::new().expect("Failed to create mDNS daemon");
        println!("mDNS Discovery: Browsing for {}", SERVICE_TYPE);
        let receiver = daemon.browse(SERVICE_TYPE).expect("Failed to browse services");

        std::thread::spawn(move || {
            println!("mDNS Discovery: Listening for services...");
            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        println!("mDNS Discovery: Found service - {}", info.get_fullname());
                        println!("mDNS Discovery: Addresses: {:?}", info.get_addresses());
                        println!("mDNS Discovery: Port: {}", info.get_port());
                        println!("mDNS Discovery: Properties: {:?}", info.get_properties());

                        let role = info
                            .get_properties()
                            .get("role")
                            .map(|v| v.val_str().to_string())
                            .unwrap_or_else(|| "unknown".to_string());

                        let name = info
                            .get_properties()
                            .get("name")
                            .map(|v| v.val_str().to_string())
                            .unwrap_or_else(|| info.get_fullname().to_string());

                        if let Some(addr) = info.get_addresses().iter().next() {
                            let service = DiscoveredService {
                                name: name.clone(),
                                role: role.clone(),
                                host: addr.to_string(),
                                port: info.get_port(),
                            };
                            println!("mDNS Discovery: Adding service {} ({}) at {}:{}", name, role, addr, info.get_port());
                            on_service_found(service);
                        } else {
                            println!("mDNS Discovery: No addresses found for service!");
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        println!("mDNS Discovery: Service removed - {}", fullname);
                    }
                    ServiceEvent::ServiceFound(_, fullname) => {
                        println!("mDNS Discovery: Service found (not yet resolved) - {}", fullname);
                    }
                    ServiceEvent::SearchStarted(_) => {
                        println!("mDNS Discovery: Search started");
                    }
                    ServiceEvent::SearchStopped(_) => {
                        println!("mDNS Discovery: Search stopped");
                    }
                }
            }
            println!("mDNS Discovery: Listener thread ended");
        });

        Self { daemon }
    }
}

impl Drop for ServiceDiscovery {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}
