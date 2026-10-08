use serde::{Deserialize, Serialize};

/// How the desktop reaches the phone (ARCHITECTURE.md §"Transport layer").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// Direct HTTP over Wi-Fi/LAN, discovered via mDNS.
    #[default]
    Wifi,
    /// `adb forward tcp:0 tcp:8080` → `http://127.0.0.1:<port>`.
    Usb,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Wifi => "Wi-Fi",
            Transport::Usb => "USB",
        }
    }
}

/// Connection lifecycle state for a discovered device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceStatus {
    /// Seen by discovery, not yet connected.
    #[default]
    Found,
    /// Authenticated and responding.
    Connected,
    /// Lost (Wi-Fi drop / USB unplug); awaiting rediscovery.
    Offline,
}

/// A phone exposing the Android18 HTTP API.
///
/// A device is — by design — nothing more than a base URL plus a token;
/// every transport is only a different way to produce that base URL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub model: String,
    /// Desktop-owned fact (the phone cannot know how it is reached): the
    /// `/info` response never carries it, so `HttpDevice` fills it in from
    /// the base URL after parsing.
    #[serde(default)]
    pub transport: Transport,
    pub base_url: String,
    /// Auth token sent as `X-Auth`. Persisted in the desktop state file.
    ///
    /// `#[serde(default)]` so `GET /info` responses — which never echo the
    /// token back — deserialize without it.
    #[serde(default)]
    pub token: String,
    /// Desktop-owned lifecycle state; `/info` never sends it and
    /// `HttpDevice` stamps `Connected` after a successful handshake.
    #[serde(default)]
    pub status: DeviceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adb_serial: Option<String>,
    /// Desktop-owned, derived from the base URL: the phone always listens
    /// on 8080, but the desktop's `adb forward` tunnel port differs.
    #[serde(default)]
    pub port: u16,
    pub storage_used_bytes: u64,
    pub storage_total_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_percent: Option<u8>,
    pub android_version: String,
}

impl Device {
    /// Transport-agnostic constructor with neutral defaults.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            id: String::new(),
            name: "Android Phone".to_string(),
            model: "Unknown model".to_string(),
            transport: Transport::Wifi,
            base_url: base_url.into(),
            token: token.into(),
            status: DeviceStatus::Found,
            ip_address: None,
            port: 8080,
            adb_serial: None,
            storage_used_bytes: 0,
            storage_total_bytes: 0,
            battery_percent: None,
            android_version: String::new(),
        }
    }

    /// Compact endpoint label for the top bar / connect dialog.
    pub fn endpoint_label(&self) -> String {
        match self.transport {
            Transport::Wifi => self
                .ip_address
                .clone()
                .map(|ip| format!("{ip}:{}", self.port))
                .unwrap_or_else(|| self.base_url.clone()),
            Transport::Usb => format!("adb:tcp:{}", self.port),
        }
    }

    /// Human storage summary, e.g. `68 GB / 256 GB`.
    pub fn storage_label(&self) -> String {
        let used = (self.storage_used_bytes as f64 / 1_073_741_824.).round() as u64;
        let total = (self.storage_total_bytes as f64 / 1_073_741_824.).round() as u64;
        format!("{used} GB / {total} GB")
    }

    /// Storage used as a fraction in `0.0..=1.0`.
    pub fn storage_fraction(&self) -> f32 {
        if self.storage_total_bytes == 0 {
            0.0
        } else {
            (self.storage_used_bytes as f32 / self.storage_total_bytes as f32).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_label_by_transport() {
        let mut d = Device::new("http://192.168.1.142:8080", "0123456789abcdef");
        d.ip_address = Some("192.168.1.142".into());
        assert_eq!(d.endpoint_label(), "192.168.1.142:8080");

        d.transport = Transport::Usb;
        d.port = 43121;
        assert_eq!(d.endpoint_label(), "adb:tcp:43121");
    }

    #[test]
    fn storage_fraction_clamped() {
        let mut d = Device::new("http://x", "0123456789abcdef");
        d.storage_total_bytes = 256;
        d.storage_used_bytes = 128;
        assert!((d.storage_fraction() - 0.5).abs() < f32::EPSILON);
        d.storage_used_bytes = 999;
        assert_eq!(d.storage_fraction(), 1.0);
        d.storage_total_bytes = 0;
        assert_eq!(d.storage_fraction(), 0.0);
    }

    #[test]
    fn parses_phone_info_wire_json() {
        // Byte-for-byte shape of the emulator's live `GET /info` body: the
        // phone never sends transport/status/port (desktop-owned facts) and
        // kotlinx's `encodeDefaults = false` strips other default-valued
        // DTO fields. This is the regression test for the "malformed /info
        // payload: missing field `transport`" connect failure.
        let json = concat!(
            r#"{"id":"android-0c6e33358b59","name":"sdk_gphone64_arm64","#,
            r#""model":"sdk_gphone64_arm64","base_url":"http://10.0.2.15:8080","#,
            r#""storage_used_bytes":2105266176,"storage_total_bytes":6228115456,"#,
            r#""battery_percent":100,"android_version":"16","ip_address":"10.0.2.15"}"#,
        );
        let device: Device = serde_json::from_str(json).expect("parse /info");
        assert_eq!(device.id, "android-0c6e33358b59");
        assert_eq!(device.storage_total_bytes, 6_228_115_456);
        assert_eq!(device.ip_address.as_deref(), Some("10.0.2.15"));
        // Desktop-owned fields default until `HttpDevice` stamps them.
        assert_eq!(device.transport, Transport::Wifi);
        assert_eq!(device.status, DeviceStatus::Found);
        assert_eq!(device.port, 0);
        assert_eq!(device.token, "");
    }

    #[test]
    fn device_serde_round_trip() {
        // `state.json` last-device persistence serializes the full struct.
        let mut device = Device::new("http://127.0.0.1:8186", "0123456789ab");
        device.id = "adb:emulator-5554".to_string();
        device.transport = Transport::Usb;
        device.port = 8186;
        device.status = DeviceStatus::Connected;
        device.adb_serial = Some("emulator-5554".to_string());
        let json = serde_json::to_string(&device).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Device>(&json).expect("deserialize"),
            device
        );
    }
}
