use crate::DiscoveredService;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const SERVICE_TYPE: &str = "_phototransfer._tcp.local.";

pub struct ServiceDiscovery {
    daemon: ServiceDaemon,
    stop_flag: Arc<AtomicBool>,
}

impl ServiceDiscovery {
    pub fn new<F, R>(on_service_found: F, on_service_removed: R) -> Self
    where
        F: Fn(DiscoveredService) + Send + Sync + 'static,
        R: Fn(String) + Send + Sync + 'static,
    {
        println!("mDNS Discovery: Starting daemon...");
        let daemon = ServiceDaemon::new().expect("Failed to create mDNS daemon");
        let stop_flag = Arc::new(AtomicBool::new(false));

        Self::start_browse(&daemon, stop_flag.clone(), on_service_found, on_service_removed);

        Self { daemon, stop_flag }
    }

    fn start_browse<F, R>(
        daemon: &ServiceDaemon,
        stop_flag: Arc<AtomicBool>,
        on_service_found: F,
        on_service_removed: R,
    ) where
        F: Fn(DiscoveredService) + Send + Sync + 'static,
        R: Fn(String) + Send + Sync + 'static,
    {
        println!("mDNS Discovery: Browsing for {}", SERVICE_TYPE);
        let receiver = daemon.browse(SERVICE_TYPE).expect("Failed to browse services");

        std::thread::spawn(move || {
            println!("mDNS Discovery: Listening for services...");
            while !stop_flag.load(Ordering::Relaxed) {
                // Use recv_timeout pentru a putea verifica stop_flag periodic
                match receiver.recv_timeout(std::time::Duration::from_millis(500)) {
                    Ok(event) => {
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
                                on_service_removed(fullname);
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
                    Err(_) => {
                        // Timeout sau disconnected - verifică stop_flag și continuă
                    }
                }
            }
            println!("mDNS Discovery: Listener thread ended");
        });
    }

    /// Trimite un nou browse query pentru a descoperi servicii noi
    pub fn refresh(&self) {
        println!("mDNS Discovery: Refreshing browse query...");
        // Re-browse pentru a forța o nouă căutare
        if let Err(e) = self.daemon.browse(SERVICE_TYPE) {
            eprintln!("mDNS Discovery: Failed to refresh: {}", e);
        }
    }

    /// Oprește discovery-ul curent
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

impl Drop for ServiceDiscovery {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        let _ = self.daemon.shutdown();
    }
}
