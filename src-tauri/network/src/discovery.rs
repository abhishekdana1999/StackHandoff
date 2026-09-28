//! mDNS/DNS-SD device discovery

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
// Only the tests write to a socket; the production paths only read.
#[cfg(test)]
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};
use workspace_clone_core::{device::*, NetworkError, Result};

use crate::wire::PROTOCOL_VERSION;

/// The mDNS service type this app registers and browses.
///
/// One constant for both halves on purpose: registering one type and parsing
/// another's suffix is how a device ends up listed under a name the user never
/// gave it.
pub const SERVICE_TYPE: &str = "_workspace-clone._tcp.local.";

/// Discovery service for finding paired devices on LAN
pub struct DiscoveryService {
    daemon: Option<ServiceDaemon>,
    discovered_devices: Arc<RwLock<HashMap<String, DiscoveredDevice>>>,
    event_sender: Option<mpsc::UnboundedSender<DiscoveryEvent>>,
    local_device_id: String,
    service_name: String,
    /// This device's Noise static public key, base64, as advertised in the TXT
    /// record.
    local_static_key: String,
}

/// Discovery events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryEvent {
    DeviceFound(DiscoveredDevice),
    DeviceLost(String),
    DeviceUpdated(DiscoveredDevice),
    Error(String),
}

impl DiscoveryService {
    /// Start discovery, advertising this device's Noise static key.
    ///
    /// The key is what makes a discovered device usable: Noise_IK needs the
    /// peer's static key before it can start, and the safety number the user
    /// compares is derived from it. Publishing nothing here -- which is what this
    /// used to do -- produced devices that appeared in the UI and then could not
    /// be connected to at all.
    pub fn new(
        local_device_id: String,
        service_name: String,
        local_static_key: &str,
    ) -> Self {
        Self {
            daemon: None,
            discovered_devices: Arc::new(RwLock::new(HashMap::new())),
            event_sender: None,
            local_device_id,
            service_name,
            local_static_key: local_static_key.to_string(),
        }
    }

    /// Start discovery service.
    ///
    /// `port` is the port the transfer listener is actually bound to. It has to
    /// be the real one: advertising a fixed default while listening on an
    /// ephemeral port produces a device that looks reachable and is not.
    pub async fn start(
        &mut self,
        event_sender: mpsc::UnboundedSender<DiscoveryEvent>,
        port: u16,
    ) -> Result<()> {
        self.event_sender = Some(event_sender);

        // Create mDNS daemon
        let daemon = ServiceDaemon::new().map_err(|e| NetworkError::Discovery(e.to_string()))?;

        // Register local service
        let service_type = SERVICE_TYPE;

        // The mDNS instance is named after the *display name*, not the device id.
        //
        // This used to be `&self.local_device_id`, which meant the `service_name`
        // passed to `new` was stored and never used. Two consequences, both bad:
        // a device called "Alex's MacBook" appeared in every network browser as a
        // row of base64, and a hostname containing a character mDNS forbids (a
        // space is fine, but a `.` is not) produced an instance name that would
        // not resolve on the peer's side.
        //
        // The device id travels in the TXT record, which is where a machine-
        // readable identity belongs; the instance name is what a person reads in
        // a network browser.
        let instance_name = &self.service_name;

        let service = ServiceInfo::new(
            service_type,
            instance_name,
            &format!("{}.local.", instance_name),
            &[] as &[std::net::IpAddr],
            port,
            Some(std::collections::HashMap::from([
                ("device_id".to_string(), self.local_device_id.clone()),
                ("protocol_version".to_string(), PROTOCOL_VERSION.to_string()),
                ("os".to_string(), std::env::consts::OS.to_string()),
                ("app_version".to_string(), env!("CARGO_PKG_VERSION").to_string()),
                // The whole point of the record.
                ("static_key".to_string(), self.local_static_key.clone()),
            ])),
        )
        .map_err(|e| NetworkError::Discovery(e.to_string()))?
        .enable_addr_auto();

        daemon
            .register(service)
            .map_err(|e| NetworkError::Discovery(e.to_string()))?;

        // Start browsing for services
        let receiver = daemon
            .browse(service_type)
            .map_err(|e| NetworkError::Discovery(e.to_string()))?;

        let discovered_devices = self.discovered_devices.clone();
        let local_device_id = self.local_device_id.clone();
        let event_sender = self.event_sender.clone();

        tokio::spawn(async move {
            while let Ok(event) = receiver.recv_async().await {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        if let Some(device) = parse_discovered_device(&info) {
                            // Skip self
                            if device.device_id == local_device_id {
                                continue;
                            }

                            let mut devices = discovered_devices.write().await;
                            let is_new = !devices.contains_key(&device.device_id);
                            devices.insert(device.device_id.clone(), device.clone());

                            if let Some(sender) = &event_sender {
                                if is_new {
                                    let _ = sender.send(DiscoveryEvent::DeviceFound(device));
                                } else {
                                    let _ = sender.send(DiscoveryEvent::DeviceUpdated(device));
                                }
                            }
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, name) => {
                        let mut devices = discovered_devices.write().await;
                        if devices.remove(&name).is_some() {
                            if let Some(sender) = &event_sender {
                                let _ = sender.send(DiscoveryEvent::DeviceLost(name));
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        self.daemon = Some(daemon);
        info!("Discovery service started");
        Ok(())
    }

    /// Stop discovery service
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(daemon) = self.daemon.take() {
            daemon
                .shutdown()
                .map_err(|e| NetworkError::Discovery(e.to_string()))?;
        }
        info!("Discovery service stopped");
        Ok(())
    }

    /// Get currently discovered devices
    pub async fn get_devices(&self) -> Vec<DiscoveredDevice> {
        let devices = self.discovered_devices.read().await;
        devices.values().cloned().collect()
    }

    /// Get a specific discovered device
    pub async fn get_device(&self, device_id: &str) -> Option<DiscoveredDevice> {
        let devices = self.discovered_devices.read().await;
        devices.get(device_id).cloned()
    }

    /// Put a device into the map as if it had just been resolved over mDNS.
    ///
    /// Test-only, and it exists to assert a property that is otherwise easy to
    /// break silently: that `get_device` reads the same map the browse loop
    /// writes. A send failed in the field because it consulted a *different*
    /// service's map, one that nothing ever populated, while the UI read this
    /// one and showed the device as present. Nothing in the types connects the
    /// two, so a test that only exercises the transport cannot notice -- it hands
    /// the peer record in directly and never asks where a real one comes from.
    #[cfg(test)]
    pub async fn insert_for_test(&self, device: DiscoveredDevice) {
        self.discovered_devices
            .write()
            .await
            .insert(device.device_id.clone(), device);
    }
}

/// Turn an mDNS record into a device this build can actually connect to.
///
/// A record missing the device id, the protocol version, or the Noise key is
/// not usable, so it is rejected rather than surfaced as a device that will fail
/// later. Showing a device in the list and then refusing to send to it is worse
/// than not showing it: the user has no way to tell the difference.
fn parse_discovered_device(info: &ServiceInfo) -> Option<DiscoveredDevice> {
    let properties = info.get_properties();

    let device_id = properties.get("device_id")?.val_str().trim().to_string();
    if device_id.is_empty() {
        return None;
    }

    let protocol_version: u32 = properties
        .get("protocol_version")?
        .val_str()
        .trim()
        .parse()
        .ok()?;

    // Noise_IK cannot start without it.
    let static_public_key = properties.get("static_key")?.val_str().trim().to_string();
    if static_public_key.is_empty() {
        warn!("Ignoring {device_id}: it advertises no Noise key, so it cannot be authenticated");
        return None;
    }

    // A record whose key this build cannot read is also unusable. Checking here
    // means the user is never offered a device that cannot be connected to.
    if workspace_clone_crypto::noise::PublicKey::from_base64(&static_public_key).is_err() {
        warn!("Ignoring {device_id}: its advertised key cannot be read");
        return None;
    }

    let addresses: Vec<String> = info
        .get_addresses()
        .iter()
        .map(|ip| ip.to_string())
        .collect();

    // An empty address list is not grounds for hiding the device: mDNS resolves
    // addresses lazily, so a record can be read before its addresses arrive. The
    // transfer layer refuses to connect with a clear message instead.

    Some(DiscoveredDevice {
        device_id,
        name: instance_display_name(info.get_fullname(), SERVICE_TYPE),
        os: properties
            .get("os")
            .map(|s| s.val_str().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string()),
        app_version: properties
            .get("app_version")
            .map(|s| s.val_str().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "0.0.0".to_string()),
        protocol_version,
        addresses,
        port: info.get_port(),
        capabilities: DeviceCapabilities::default(),
        static_public_key,
        last_seen: chrono::Utc::now().into(),
    })
}

/// The display name a human chose, recovered from an mDNS full name.
///
/// An mDNS full name is `{instance}.{service type}.{domain}`, so
/// `BISWAJITA._workspace-clone._tcp.local.` is one device called `BISWAJITA`.
/// Only the instance label is a name; the rest is DNS plumbing, and a device
/// list is read by people looking for their own laptop.
///
/// This used to strip the trailing dot and the `.local` domain and stop there,
/// which left the *service type* on the end -- so the laptop appeared as
/// `BISWAJITA._workspace-clone._tcp`, which is the opposite of recognising it.
/// The comment beside the old code said the goal was a name that helps a person
/// recognise their own machine, so this is the bug the comment was warning about
/// rather than the bug it described.
///
/// The whole service suffix is removed rather than a `.local` substring,
/// because an instance label may legitimately end in one: a Mac whose display
/// name is `ABHISHEKs-MacBook-Air.local` must keep it. Stripping `.local` as a
/// suffix would quietly rename that device to something the user never chose.
///
/// The suffix is matched case-insensitively because mDNS names are, and a peer
/// is free to register its type in any case it likes.
pub(crate) fn instance_display_name(fullname: &str, service_type: &str) -> String {
    // `service_type` carries its own trailing dot in the constant it comes from;
    // build the suffix that actually appears in a full name.
    let suffix = format!(".{service_type}");

    if fullname.len() > suffix.len() {
        let (label, rest) = fullname.split_at(fullname.len() - suffix.len());
        if rest.eq_ignore_ascii_case(&suffix) {
            return label.to_string();
        }
    }

    // No recognisable suffix. Better a slightly noisy name than an empty one,
    // since an empty name leaves a device the user cannot tell apart from
    // another nameless device on the same network.
    fullname.trim_end_matches('.').to_string()
}

/// Ask a device at a known address who it is.
///
/// This is the bootstrap for pairing without mDNS: the reply is an
/// *unauthenticated* claim about identity, which is why it is not enough on its
/// own. The caller must confirm the resulting safety number with the person at
/// the other device before any workspace is sent, and the transfer layer refuses
/// to connect to a device that did not supply a key.
///
/// Returns an error rather than a guess, so a user who types a wrong address is
/// told so instead of watching a spinner.
pub async fn connect_manual(address: &str, port: u16) -> Result<DiscoveredDevice> {
    let mut stream = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::net::TcpStream::connect((address, port)),
    )
    .await
    .map_err(|_| {
        NetworkError::Connection(format!("Nothing answered at {address}:{port} within 5 seconds"))
    })?
    .map_err(|e| NetworkError::Connection(format!("Could not reach {address}:{port}: {e}")))?;

    let mut parser = workspace_clone_crypto::noise::FrameParser::new();
    let mut buffer = vec![0u8; 4096];

    let read = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read(&mut buffer),
    )
    .await
    .map_err(|_| {
        NetworkError::Connection(format!(
            "{address}:{port} accepted the connection but sent no identity"
        ))
    })?
    .map_err(|e| NetworkError::Connection(format!("Read failed: {e}")))?;

    if read == 0 {
        return Err(NetworkError::Connection(format!(
            "{address}:{port} closed the connection without sending an identity"
        ))
        .into());
    }

    let frame = parser
        .feed(&buffer[..read])?
        .into_iter()
        .next()
        .ok_or_else(|| {
            NetworkError::Connection(format!(
                "{address}:{port} did not send a complete identity frame"
            ))
        })?;

    let claim: ManualPeerClaim = serde_json::from_slice(&frame).map_err(|e| {
        NetworkError::Protocol(format!("{address}:{port} sent an unreadable identity: {e}"))
    })?;

    if claim.static_key.is_empty() {
        return Err(NetworkError::Authentication(format!(
            "{address}:{port} did not advertise a key, so it cannot be authenticated"
        ))
        .into());
    }
    if workspace_clone_crypto::noise::PublicKey::from_base64(&claim.static_key).is_err() {
        return Err(NetworkError::Authentication(format!(
            "{address}:{port} advertised a key this build cannot read"
        ))
        .into());
    }

    Ok(DiscoveredDevice {
        device_id: claim.device_id,
        name: claim.name,
        os: claim.os,
        app_version: claim.app_version,
        protocol_version: claim.protocol_version,
        addresses: vec![address.to_string()],
        port,
        capabilities: DeviceCapabilities::default(),
        static_public_key: claim.static_key,
        last_seen: chrono::Utc::now().into(),
    })
}

/// The identity a device volunteers when asked directly.
///
/// Deliberately a claim, not proof. Nothing here is authenticated until the
/// safety number is confirmed by the user.
#[derive(Debug, Clone, serde::Deserialize)]
struct ManualPeerClaim {
    device_id: String,
    name: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    app_version: String,
    #[serde(default)]
    protocol_version: u32,
    #[serde(default)]
    static_key: String,
}

/// Build the identity claim this device would send in response to a probe.
pub fn manual_peer_claim(device_id: &str, name: &str, static_key: &str) -> serde_json::Value {
    serde_json::json!({
        "device_id": device_id,
        "name": name,
        "os": std::env::consts::OS,
        "app_version": env!("CARGO_PKG_VERSION"),
        "protocol_version": PROTOCOL_VERSION,
        "static_key": static_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> String {
        use workspace_clone_crypto::noise::KeyPair;
        workspace_clone_crypto::noise::PublicKey::from_bytes(
            KeyPair::generate().unwrap().public_bytes(),
        )
        .unwrap()
        .to_base64()
    }

    fn properties(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn service_info(props: std::collections::HashMap<String, String>, port: u16) -> ServiceInfo {
        ServiceInfo::new(
            "_workspace-clone._tcp.local.",
            "peer-1",
            "peer-1.local.",
            &[] as &[std::net::IpAddr],
            port,
            Some(props),
        )
        .unwrap()
        .enable_addr_auto()
    }

    #[test]
    fn a_full_record_is_parsed() {
        let k = key();
        let device = parse_discovered_device(&service_info(
            properties(&[
                ("device_id", "peer-1"),
                ("protocol_version", "1"),
                ("static_key", &k),
                ("os", "linux"),
                ("app_version", "0.1.0"),
            ]),
            47890,
        ))
        .expect("a complete record must parse");

        assert_eq!(device.device_id, "peer-1");
        assert_eq!(device.static_public_key, k);
        assert_eq!(device.os, "linux");
        assert_eq!(device.app_version, "0.1.0");
        assert_eq!(device.port, 47890);
        // Addresses resolve asynchronously over mDNS, so a record read before
        // they arrive is still a usable one. The port is what makes a device
        // connectable, and it must survive the round trip.
        assert_eq!(device.protocol_version, 1);
    }

    #[test]
    fn a_record_with_no_resolved_addresses_is_still_parsed() {
        // Hiding the device here would leave the user with no explanation; the
        // transfer layer reports the missing address when a send is attempted.
        let device = parse_discovered_device(&service_info(
            properties(&[
                ("device_id", "peer-1"),
                ("protocol_version", "1"),
                ("static_key", &key()),
            ]),
            47890,
        ))
        .expect("a record with a key is usable even before its addresses resolve");

        assert!(device.addresses.is_empty());
        assert_eq!(device.port, 47890);
    }

    #[test]
    fn a_record_without_a_key_is_rejected() {
        // The defect this replaces: such a device appeared in the UI and then
        // could not be connected to.
        assert!(
            parse_discovered_device(&service_info(
                properties(&[
                    ("device_id", "peer-1"),
                    ("protocol_version", "1"),
                ]),
                47890,
            ))
            .is_none()
        );
    }

    #[test]
    fn a_record_with_an_empty_key_is_rejected() {
        assert!(
            parse_discovered_device(&service_info(
                properties(&[
                    ("device_id", "peer-1"),
                    ("protocol_version", "1"),
                    ("static_key", "   "),
                ]),
                47890,
            ))
            .is_none()
        );
    }

    #[test]
    fn a_record_with_an_unreadable_key_is_rejected() {
        assert!(
            parse_discovered_device(&service_info(
                properties(&[
                    ("device_id", "peer-1"),
                    ("protocol_version", "1"),
                    ("static_key", "not-a-key"),
                ]),
                47890,
            ))
            .is_none()
        );
    }

    #[test]
    fn a_record_without_a_device_id_is_rejected() {
        assert!(
            parse_discovered_device(&service_info(
                properties(&[("protocol_version", "1"), ("static_key", &key())]),
                47890,
            ))
            .is_none()
        );
    }

    #[test]
    fn a_record_with_a_blank_device_id_is_rejected() {
        assert!(
            parse_discovered_device(&service_info(
                properties(&[
                    ("device_id", "  "),
                    ("protocol_version", "1"),
                    ("static_key", &key()),
                ]),
                47890,
            ))
            .is_none()
        );
    }

    #[test]
    fn a_record_with_an_unparseable_protocol_version_is_rejected() {
        assert!(
            parse_discovered_device(&service_info(
                properties(&[
                    ("device_id", "peer-1"),
                    ("protocol_version", "not a number"),
                    ("static_key", &key()),
                ]),
                47890,
            ))
            .is_none()
        );
    }

    #[test]
    fn the_display_name_is_not_a_dns_name() {
        let k = key();
        let device = parse_discovered_device(&service_info(
            properties(&[
                ("device_id", "peer-1"),
                ("protocol_version", "1"),
                ("static_key", &k),
            ]),
            47890,
        ))
        .unwrap();

        assert!(
            !device.name.ends_with('.'),
            "a trailing dot is a DNS artefact, not a name: {:?}",
            device.name
        );
        assert!(!device.name.contains(".local"), "got {:?}", device.name);

        // The service type is the part this assertion used to miss, and it is
        // the part that was actually wrong: the device was listed as
        // `peer-1._workspace-clone._tcp`, which is a service type wearing a
        // device's name. Checking only for a trailing dot and `.local` left this
        // green while the name was unusable, so the exact expected value is
        // asserted rather than two properties that happen to hold.
        assert_eq!(
            device.name, "peer-1",
            "the name must be the instance label alone, with no service type"
        );
    }

    #[test]
    fn a_display_name_that_itself_ends_in_local_is_preserved() {
        // A Mac named `ABHISHEKs-MacBook-Air.local` really does advertise that
        // label. Stripping `.local` as a suffix would rename a device to
        // something the user never chose, which is the same class of bug as
        // leaving the service type on.
        assert_eq!(
            instance_display_name(
                "ABHISHEKs-MacBook-Air.local._workspace-clone._tcp.local.",
                SERVICE_TYPE
            ),
            "ABHISHEKs-MacBook-Air.local"
        );
    }

    #[test]
    fn the_service_type_is_stripped_whatever_its_case() {
        // mDNS names are case-insensitive and a peer may register its type in
        // any case, so a case-sensitive suffix match would leave that peer's
        // name carrying its own plumbing.
        assert_eq!(
            instance_display_name("BISWAJITA._Workspace-Clone._TCP.local.", SERVICE_TYPE),
            "BISWAJITA"
        );
    }

    #[test]
    fn an_unrecognised_full_name_still_yields_something_usable() {
        // A name is never a good reason to lose a device. Better a noisy name
        // than an empty one, since two nameless devices are indistinguishable.
        assert_eq!(
            instance_display_name("something-else.local.", SERVICE_TYPE),
            "something-else.local"
        );
        assert_eq!(instance_display_name("bare", SERVICE_TYPE), "bare");
    }

    #[test]
    fn a_device_whose_label_is_only_the_service_type_is_not_reduced_to_nothing() {
        // Guards the `len >` comparison: a full name that is nothing but the
        // suffix would otherwise produce an empty label, and an empty name is
        // worse than a noisy one.
        let degenerate = SERVICE_TYPE.to_string();
        let name = instance_display_name(&degenerate, SERVICE_TYPE);
        assert!(!name.is_empty(), "a device must never be listed as blank");
    }

    #[tokio::test]
    async fn manual_pairing_reports_a_dead_address() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let error = connect_manual("127.0.0.1", port).await.unwrap_err().to_string();
        assert!(
            error.contains("Nothing answered") || error.contains("Could not reach"),
            "got: {error}"
        );
    }

    #[tokio::test]
    async fn manual_pairing_reports_a_silent_peer() {
        // A peer that accepts and then says nothing must not look like success.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 16];
            let _ = stream.read(&mut buf).await;
        });

        let error = connect_manual("127.0.0.1", port).await.unwrap_err().to_string();
        assert!(
            error.contains("sent no identity") || error.contains("closed the connection"),
            "got: {error}"
        );
    }

    #[tokio::test]
    async fn manual_pairing_reports_an_immediate_close() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream);
        });

        let error = connect_manual("127.0.0.1", port).await.unwrap_err().to_string();
        assert!(error.contains("closed the connection"), "got: {error}");
    }

    #[tokio::test]
    async fn manual_pairing_reads_a_peer_identity() {
        let k = key();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let claim = manual_peer_claim("peer-9", "Alice's laptop", &k);

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let framed = workspace_clone_crypto::noise::frame_message(claim.to_string().as_bytes());
            let _ = stream.write_all(&framed).await;
        });

        let device = connect_manual("127.0.0.1", port).await.unwrap();

        assert_eq!(device.device_id, "peer-9");
        assert_eq!(device.name, "Alice's laptop");
        assert_eq!(device.static_public_key, k);
        assert_eq!(device.port, port);
        assert_eq!(device.addresses, vec!["127.0.0.1".to_string()]);
    }

    #[tokio::test]
    async fn manual_pairing_refuses_a_peer_with_no_key() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // An old build that does not advertise a key.
        let claim = serde_json::json!({ "device_id": "old", "name": "Old" });

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let framed = workspace_clone_crypto::noise::frame_message(claim.to_string().as_bytes());
            let _ = stream.write_all(&framed).await;
        });

        let error = connect_manual("127.0.0.1", port).await.unwrap_err().to_string();
        assert!(error.contains("cannot be authenticated"), "got: {error}");
    }

    #[tokio::test]
    async fn manual_pairing_reports_garbage() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let framed = workspace_clone_crypto::noise::frame_message(b"not json at all");
            let _ = stream.write_all(&framed).await;
        });

        let error = connect_manual("127.0.0.1", port).await.unwrap_err().to_string();
        assert!(error.contains("unreadable identity"), "got: {error}");
    }

    #[test]
    fn the_claim_this_device_sends_carries_its_key() {
        let k = key();
        let claim = manual_peer_claim("me", "My laptop", &k);

        assert_eq!(claim["static_key"], serde_json::json!(k));
        assert_eq!(claim["device_id"], serde_json::json!("me"));
        assert_eq!(claim["protocol_version"], serde_json::json!(PROTOCOL_VERSION));
    }

    #[test]
    fn a_discovery_event_round_trips() {
        let json = serde_json::to_string(&DiscoveryEvent::Error("test".to_string())).unwrap();
        assert!(json.contains("test"));
    }
}
