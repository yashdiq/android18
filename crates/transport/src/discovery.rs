//! Device discovery: mDNS (`_android18._tcp.local.`) for Wi-Fi phones,
//! `adb devices -l` for USB-attached ones. Discovery only ever produces a
//! **base URL + identity**; pairing tokens live in [`crate::state`].

use std::net::IpAddr;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use mdns_sd::{RecvTimeoutError, ServiceDaemon, ServiceEvent};

/// The mDNS service type the phone service registers.
pub const SERVICE_TYPE: &str = "_android18._tcp.local.";
/// Port the phone service listens on (both Wi-Fi and USB-forwarded).
pub const PHONE_PORT: u16 = 8080;

/// How a device was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    /// Bonjour/mDNS on the LAN.
    Mdns,
    /// USB via `adb forward` (base URL is `127.0.0.1`).
    Usb,
}

/// One discovered phone, before pairing/connection.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredDevice {
    pub id: String,
    pub name: String,
    pub source: DiscoverySource,
    /// LAN address (mDNS only; USB devices tunnel through localhost).
    pub ip: Option<IpAddr>,
    pub port: u16,
    pub adb_serial: Option<String>,
    /// The phone advertised `pair=code`: connecting wants its 6-char code
    /// (otherwise the phone shows an allow prompt).
    pub code_pairing: bool,
}

impl DiscoveredDevice {
    /// The base URL a [`HttpDevice`](crate::HttpDevice) would connect to.
    /// USB devices need [`adb_forward`] first, which fills `port`.
    pub fn base_url(&self) -> Option<String> {
        match self.source {
            DiscoverySource::Mdns => self.ip.map(|ip| format!("http://{ip}:{}", self.port)),
            DiscoverySource::Usb => {
                if self.port == 0 {
                    None
                } else {
                    Some(format!("http://127.0.0.1:{}", self.port))
                }
            }
        }
    }
}

/// One pass over `adb devices -l`, split by device state. Only serials in
/// the `device` (authorized) state can be forwarded and connected;
/// `unauthorized` serials are plugged in but their screen is still showing
/// the "Allow USB debugging?" confirmation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdbScan {
    /// Authorized `(serial, model)` pairs, connectable via [`adb_forward`].
    pub devices: Vec<(String, String)>,
    /// Serials waiting for the on-device USB-debugging prompt.
    pub unauthorized: Vec<String>,
}

/// Everything a full discovery pass found: connectable devices plus the
/// unauthorized USB serials, so the UI can hint "accept the prompt" instead
/// of offering dead candidates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiscoveryReport {
    pub devices: Vec<DiscoveredDevice>,
    pub unauthorized: Vec<String>,
}

/// Runs both discovery sources (mDNS first, then USB). Never fails: sources
/// without a daemon/`adb` are skipped, and unauthorized phones are reported
/// separately instead of becoming unconnectable candidates.
pub fn discover(timeout: Duration) -> DiscoveryReport {
    let mut report = discover_usb();
    let mut devices = discover_mdns(timeout);
    devices.append(&mut report.devices);
    report.devices = devices;
    report
}

/// USB half of [`discover`]: one cheap `adb devices -l` pass, no mDNS
/// browse — what the desktop's hotplug watcher polls every few seconds.
pub fn discover_usb() -> DiscoveryReport {
    let scan = adb_scan();
    DiscoveryReport {
        devices: scan
            .devices
            .iter()
            .map(|(serial, model)| DiscoveredDevice {
                id: format!("adb:{serial}"),
                name: model.clone(),
                source: DiscoverySource::Usb,
                ip: None,
                port: 0,
                adb_serial: Some(serial.clone()),
                code_pairing: false,
            })
            .collect(),
        unauthorized: scan.unauthorized,
    }
}

/// Browses `_android18._tcp` for up to `timeout`, returning resolved
/// services with their TXT properties (`id`, `name`, `model`).
pub fn discover_mdns(timeout: Duration) -> Vec<DiscoveredDevice> {
    let mut devices = Vec::new();
    let Ok(daemon) = ServiceDaemon::new() else {
        return devices; // no mDNS daemon on this machine
    };
    let Ok(receiver) = daemon.browse(SERVICE_TYPE) else {
        return devices;
    };
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                // Prefer the first IPv4 address (LAN phones); link-local
                // scoped variants are not surfaced here.
                let Some(ip) = info.get_addresses_v4().into_iter().next() else {
                    continue;
                };
                let txt = |key: &str| {
                    info.get_property_val_str(key)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                };
                let id = txt("id").unwrap_or_else(|| info.get_fullname().to_string());
                if devices.iter().any(|d: &DiscoveredDevice| d.id == id) {
                    continue; // mDNS may re-resolve; keep the first sighting
                }
                devices.push(DiscoveredDevice {
                    name: txt("name")
                        .or_else(|| txt("model"))
                        .unwrap_or_else(|| "Android Phone".to_string()),
                    id,
                    source: DiscoverySource::Mdns,
                    ip: Some(IpAddr::V4(ip)),
                    port: info.get_port(),
                    adb_serial: None,
                    code_pairing: txt("pair").as_deref() == Some("code"),
                });
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = daemon.shutdown();
    devices
}

/// Lists USB-attached phones via `adb devices -l` (empty when `adb` is
/// missing or the daemon isn't running). Ports stay 0 until [`adb_forward`]
/// runs. Output is captured, never inherited, so adb's daemon banner and
/// error chatter cannot leak into the app's terminal.
pub fn adb_scan() -> AdbScan {
    let Ok(output) = adb_command().args(["devices", "-l"]).output() else {
        return AdbScan::default();
    };
    parse_adb_devices(&String::from_utf8_lossy(&output.stdout))
}

/// Sets up `adb forward tcp:<local> tcp:8080` for `serial`, returning the
/// local port. Ports are deterministic per serial (8180–8189) so repeat
/// connections reuse the same tunnel.
pub fn adb_forward(serial: &str) -> Result<u16, String> {
    let port: u16 = (8180 + simple_hash(serial) % 10) as u16;
    let forward = format!("tcp:{port}");
    let target = format!("tcp:{PHONE_PORT}");
    let output = adb_command()
        .args(["-s", serial, "forward", &forward, &target])
        .output()
        .map_err(|e| format!("adb forward failed to start: {e}"))?;
    if output.status.success() {
        Ok(port)
    } else {
        // Surface adb's own stderr (e.g. "device unauthorized") instead of
        // leaking it to the terminal and reporting a bare exit code.
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("adb forward exited with {}", output.status))
        } else {
            Err(stderr)
        }
    }
}

/// Package + activity the USB connect intent targets. The service itself
/// is not exported, so only the activity can be started from `adb shell`.
const PHONE_ACTIVITY: &str = "com.android18.service/.MainActivity";
/// Intent action the phone's `MainActivity` turns into an "Allow this
/// computer?" prompt.
const CONNECT_ACTION: &str = "com.android18.service.CONNECT";

/// Opens the phone's allow prompt over USB:
/// `adb shell am start -n …/.MainActivity -a …CONNECT --es desktop <name>`.
/// The phone starts its service only once the user taps Allow.
pub fn adb_launch_connect(serial: &str, desktop_name: &str) -> Result<(), String> {
    // `am` runs the argument through the device shell, so keep the name to
    // a quote-free subset and wrap it in single quotes.
    let safe: String = desktop_name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '\u{2019}'))
        .take(60)
        .collect();
    let command = format!(
        "am start -n {PHONE_ACTIVITY} -a {CONNECT_ACTION} --es desktop '{}'",
        safe.trim()
    );
    let output = adb_command()
        .args(["-s", serial, "shell", &command])
        .output()
        .map_err(|e| format!("adb shell failed to start: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && !stdout.contains("Error") {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr
        })
    }
}

/// Parses `adb devices -l` output. The state column decides usability:
/// `device` is authorized, `unauthorized` is waiting on its prompt, and
/// anything else (offline/recovery/sideload/bootloader) is ignored.
fn parse_adb_devices(output: &str) -> AdbScan {
    let mut scan = AdbScan::default();
    for line in output
        .lines()
        .skip_while(|line| line.starts_with('*')) // daemon banner
        .skip(1)
    // header: "List of devices attached"
    {
        let mut parts = line.split_whitespace();
        let Some(serial) = parts.next() else {
            continue;
        };
        if serial.is_empty() || serial.starts_with('*') {
            continue;
        }
        let state = parts.next().unwrap_or_default();
        let model = line
            .split("model:")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or("Android device");
        match state {
            "device" => scan.devices.push((serial.to_string(), model.to_string())),
            "unauthorized" => scan.unauthorized.push(serial.to_string()),
            _ => {}
        }
    }
    scan
}

/// Builds the `adb` command to run. `Command::new("adb")` only works when the
/// process inherited a shell `PATH`; launched from Finder/Dock there is none,
/// so well-known install locations are probed first.
fn adb_command() -> Command {
    match first_existing(&adb_candidates()) {
        Some(path) => Command::new(path),
        None => Command::new("adb"), // PATH fallback; errors surface upstream
    }
}

/// Absolute `adb` candidates in priority order (Android Studio SDK first).
fn adb_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for (env, suffix) in [
        ("HOME", "Library/Android/sdk/platform-tools/adb"),
        ("ANDROID_HOME", "platform-tools/adb"),
        ("ANDROID_SDK_ROOT", "platform-tools/adb"),
    ] {
        if let Ok(value) = std::env::var(env)
            && !value.is_empty()
        {
            candidates.push(PathBuf::from(value).join(suffix));
        }
    }
    candidates.extend(
        [
            "/opt/homebrew/bin/adb",
            "/usr/local/bin/adb",
            "/tmp/android-sdk/platform-tools/adb",
        ]
        .iter()
        .map(PathBuf::from),
    );
    candidates
}

/// First candidate that exists as a file.
fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.is_file()).cloned()
}

/// Stable, tiny string hash (FNV-1a) for deterministic port assignment.
fn simple_hash(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adb_devices_output() {
        let output = "List of devices attached\n\
                     HT9XYZ	device product:... model:Pixel_8\n\
                     emulator-5554	device product:... model:SDK_gphone\n\
                      \n\
                     * daemon started successfully\n";
        let scan = parse_adb_devices(output);
        assert_eq!(
            scan.devices,
            vec![
                ("HT9XYZ".to_string(), "Pixel_8".to_string()),
                ("emulator-5554".to_string(), "SDK_gphone".to_string()),
            ]
        );
        assert!(scan.unauthorized.is_empty());
    }

    #[test]
    fn separates_unauthorized_devices() {
        let output = "* daemon not running; starting now at tcp:5037\n\
                      * daemon started successfully\n\
                      List of devices attached\n\
                      HT9XYZ\tdevice product:... model:Pixel_8\n\
                      emulator-5554\tunauthorized product:... model:SDK_gphone\n\
                      emulator-5556\tdevice product:... model:SDK_gphone_x86\n\
                      0A98312B7D\toffline\n";
        let scan = parse_adb_devices(output);
        assert_eq!(scan.devices.len(), 2);
        assert_eq!(scan.devices[0].0, "HT9XYZ");
        assert_eq!(scan.devices[1].0, "emulator-5556");
        assert_eq!(scan.unauthorized, vec!["emulator-5554".to_string()]);
    }

    #[test]
    fn adb_resolver_picks_first_existing_file() {
        let dir = std::env::temp_dir();
        let existing = dir.join(format!("adb-stub-{}", std::process::id()));
        std::fs::write(&existing, b"stub").expect("write temp adb stub");
        let missing = dir.join("adb-stub-definitely-missing");
        let picked = first_existing(&[missing.clone(), existing.clone()]);
        assert_eq!(picked.as_deref(), Some(existing.as_path()));
        assert_eq!(first_existing(&[missing]), None);
        std::fs::remove_file(&existing).ok();
    }

    #[test]
    fn usb_base_url_needs_forwarded_port() {
        let mut device = DiscoveredDevice {
            id: "adb:HT9XYZ".to_string(),
            name: "Pixel 8".to_string(),
            source: DiscoverySource::Usb,
            ip: None,
            port: 0,
            adb_serial: Some("HT9XYZ".to_string()),
            code_pairing: false,
        };
        assert_eq!(device.base_url(), None);
        device.port = 8183;
        assert_eq!(device.base_url().as_deref(), Some("http://127.0.0.1:8183"));
    }

    #[test]
    fn mdns_base_url_uses_ip_and_port() {
        let device = DiscoveredDevice {
            id: "pixel-8".to_string(),
            name: "Pixel 8".to_string(),
            source: DiscoverySource::Mdns,
            ip: Some("192.168.1.42".parse().expect("valid ip")),
            port: 8080,
            adb_serial: None,
            code_pairing: true,
        };
        assert_eq!(
            device.base_url().as_deref(),
            Some("http://192.168.1.42:8080")
        );
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(simple_hash("HT9XYZ"), simple_hash("HT9XYZ"));
        assert_ne!(simple_hash("HT9XYZ"), simple_hash("other"));
    }
}
