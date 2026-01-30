use serde::{Deserialize, Serialize};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const SERVICE_TYPE: &str = "_phototransfer._tcp.local.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredService {
    pub name: String,
    pub role: String,
    pub host: String,
    pub port: u16,
}

pub struct ServiceDiscovery {
    daemon: ServiceDaemon,
    pub services: Arc<Mutex<HashMap<String, DiscoveredService>>>,
}

impl ServiceDiscovery {
    pub fn new(my_name: String) -> Self {
        println!("mDNS Discovery: Starting daemon...");
        let daemon = ServiceDaemon::new().expect("Failed to create mDNS daemon");
        let services = Arc::new(Mutex::new(HashMap::new()));
        let services_clone = Arc::clone(&services);
        let my_name_clone = my_name.clone();

        println!("mDNS Discovery: Browsing for {}", SERVICE_TYPE);
        let receiver = daemon.browse(SERVICE_TYPE).expect("Failed to browse services");

        std::thread::spawn(move || {
            println!("mDNS Discovery: Listening for services...");
            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        println!("mDNS Discovery: Found service - {}", info.get_fullname());

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

                        // Nu adăugăm pe noi înșine
                        if name == my_name_clone {
                            println!("mDNS Discovery: Skipping self ({})", name);
                            continue;
                        }

                        if let Some(addr) = info.get_addresses().iter().next() {
                            let service = DiscoveredService {
                                name: name.clone(),
                                role: role.clone(),
                                host: addr.to_string(),
                                port: info.get_port(),
                            };
                            println!(
                                "mDNS Discovery: Adding service {} ({}) at {}:{}",
                                name, role, addr, info.get_port()
                            );

                            let mut svcs = services_clone.lock().unwrap();
                            svcs.insert(name, service);
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        println!("mDNS Discovery: Service removed - {}", fullname);
                        let mut svcs = services_clone.lock().unwrap();
                        // Încercăm să găsim și să eliminăm serviciul
                        let key_to_remove: Option<String> = svcs
                            .iter()
                            .find(|(_, s)| fullname.contains(&s.name))
                            .map(|(k, _)| k.clone());

                        if let Some(key) = key_to_remove {
                            svcs.remove(&key);
                        }
                    }
                    _ => {}
                }
            }
        });

        Self { daemon, services }
    }

    pub fn get_editors(&self) -> Vec<DiscoveredService> {
        let svcs = self.services.lock().unwrap();
        // Returnează TOȚI receiver-ii (editor sau tagger) - filtrarea se face în UI dacă e nevoie
        // Astfel un editor poate trimite la alt editor, și un tagger poate trimite la editor
        svcs.values()
            .cloned()
            .collect()
    }
}

impl Drop for ServiceDiscovery {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}
