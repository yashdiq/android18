//! Desktop state at `~/Library/Application Support/Android18/state.json` —
//! one owner-only file holding everything the app remembers between
//! launches: the last connected device, USB serials, pin/color-tag
//! decorations and the pairing tokens (`device-id → token`).
//!
//! The token is a bearer credential for the phone's file API, so the file
//! is written mode `0600` (and tightened on open if an older build left it
//! wider). That is the same trust posture ADB itself uses —
//! `~/.android/adbkey` is a plaintext private key — and the phone can
//! revoke a leaked token. A keychain would add ACL-guarded encryption at
//! rest, but its per-signature ACLs made every launch flavor (bare binary,
//! dev bundle, release DMG) negotiate access dialogs; the file never
//! prompts, from any flavor.
//!
//! All access funnels through one process-wide mutex; writes are atomic
//! (tmp + rename) and only happen when the serialized state actually
//! changed, so a silent reconnect that persists nothing new costs zero
//! disk writes.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use android18_core::domain::{ColorTag, Device};
use serde::{Deserialize, Serialize};

/// Desktop-local decoration for one path (pin + optional color tag).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecorationRecord {
    pub pinned: bool,
    pub tag: Option<ColorTag>,
}

/// Everything persisted between launches, pairing tokens included.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedState {
    /// The last successfully connected device, for launch auto-reconnect.
    pub last_device: Option<Device>,
    /// `device-id → adb serial` it last connected through.
    pub usb_serials: HashMap<String, String>,
    /// `device-id → path → pin/tag`.
    pub decorations: HashMap<String, HashMap<String, DecorationRecord>>,
    /// `device-id → pairing token` (the `X-Auth` secret). BTreeMap so the
    /// serialized form is order-stable and change detection never
    /// false-fires on a rewrite.
    pub tokens: BTreeMap<String, String>,
}

impl PersistedState {
    /// Parses state JSON; corrupt files fall back to defaults.
    pub fn from_json(text: &str) -> PersistedState {
        serde_json::from_str(text).unwrap_or_default()
    }

    /// Serializes for the state file. Plain data, so a failure here is a
    /// loud invariant rather than silently lost state.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("persisted state serializes")
    }
}

/// `~/Library/Application Support/Android18/state.json` (`None` when
/// `$HOME` is unset — the store then lives in memory only).
pub fn state_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Android18")
            .join("state.json")
    })
}

static STORE: OnceLock<Mutex<StateStore>> = OnceLock::new();

/// Process-wide store. Lazy: the first access loads the file.
fn store() -> &'static Mutex<StateStore> {
    STORE.get_or_init(|| Mutex::new(StateStore::open(state_path())))
}

/// File-backed [`PersistedState`] plus its serialized shadow for change
/// detection. Only this module touches one directly.
struct StateStore {
    path: Option<PathBuf>,
    /// `true` once a state file has been read or written.
    persisted: bool,
    state: PersistedState,
    /// Serialized form of the last disk read/write.
    json: String,
}

impl StateStore {
    /// Loads the file at `path` (defaults when absent or corrupt).
    fn open(path: Option<PathBuf>) -> Self {
        let default_json = PersistedState::default().to_json();
        let Some(path) = path else {
            return Self {
                path: None,
                persisted: false,
                state: PersistedState::default(),
                json: default_json,
            };
        };
        // The file carries pairing tokens: tighten anything an older
        // build left world/group-readable.
        restrict(&path);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self {
                state: PersistedState::from_json(&text),
                json: text,
                persisted: true,
                path: Some(path),
            },
            Err(_) => Self {
                path: Some(path),
                persisted: false,
                state: PersistedState::default(),
                json: default_json,
            },
        }
    }

    /// Applies `change` and persists **iff** the serialized state changed.
    fn update(&mut self, change: impl FnOnce(&mut PersistedState)) -> Result<(), String> {
        change(&mut self.state);
        let json = self.state.to_json();
        if json == self.json {
            return Ok(());
        }
        if let Some(path) = &self.path {
            write_atomic(path, &json)?;
            self.persisted = true;
        }
        self.json = json;
        Ok(())
    }
}

/// Atomic (tmp + rename) write; creates parent directories. The tmp file
/// is `chmod 600` *before* the rename so the final state file — which
/// carries pairing tokens — is never observable with looser permissions.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("state dir: {e}"))?;
    }
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("state write: {e}"))?;
    restrict(&tmp);
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("state rename: {e}")
    })
}

/// Best-effort `chmod 600`; never fatal — a failure leaves the previous
/// mode in place and the next successful write retries.
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path)
        && meta.permissions().mode() & 0o777 != 0o600
    {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

/// The last successfully connected device, if state still has it.
pub fn load_last_device() -> Option<Device> {
    store()
        .lock()
        .expect("state store mutex")
        .state
        .last_device
        .clone()
}

/// Remembers `device` as the last successful connection.
pub fn save_last_device(device: &Device) -> Result<(), String> {
    store()
        .lock()
        .expect("state store mutex")
        .update(|state| state.last_device = Some(device.clone()))
}

/// Forgets the last connected device (used when switching devices).
pub fn clear_last_device() -> Result<(), String> {
    store()
        .lock()
        .expect("state store mutex")
        .update(|state| state.last_device = None)
}

/// The USB serial `device_id` last connected through, if any.
pub fn load_usb_serial(device_id: &str) -> Option<String> {
    store()
        .lock()
        .expect("state store mutex")
        .state
        .usb_serials
        .get(device_id)
        .cloned()
}

/// Remembers that `device_id` last connected over USB through `serial`.
pub fn save_usb_serial(device_id: &str, serial: &str) -> Result<(), String> {
    store().lock().expect("state store mutex").update(|state| {
        state
            .usb_serials
            .insert(device_id.to_string(), serial.to_string());
    })
}

/// Loads the decoration map for `device_id` (empty when never saved).
pub fn load_decorations(device_id: &str) -> Result<HashMap<String, DecorationRecord>, String> {
    Ok(store()
        .lock()
        .expect("state store mutex")
        .state
        .decorations
        .get(device_id)
        .cloned()
        .unwrap_or_default())
}

/// Saves every pin/color-tag decoration for `device_id` (one JSON blob).
pub fn save_decorations(
    device_id: &str,
    map: &HashMap<String, DecorationRecord>,
) -> Result<(), String> {
    store().lock().expect("state store mutex").update(|state| {
        state.decorations.insert(device_id.to_string(), map.clone());
    })
}

/// The stored pairing token for `device_id`, if any.
pub fn load_token(device_id: &str) -> Option<String> {
    store()
        .lock()
        .expect("state store mutex")
        .state
        .tokens
        .get(device_id)
        .cloned()
}

/// Stores the pairing token for `device_id`. Change detection in
/// [`StateStore::update`] already skips identical writes, so a silent
/// reconnect that re-saves the same token never touches disk.
pub fn save_token(device_id: &str, token: &str) -> Result<(), String> {
    store().lock().expect("state store mutex").update(|state| {
        state
            .tokens
            .insert(device_id.to_string(), token.to_string());
    })
}

/// Deletes the stored pairing token for `device_id` (absent tokens are
/// fine — a no-op write never reaches disk).
pub fn delete_token(device_id: &str) -> Result<(), String> {
    store().lock().expect("state store mutex").update(|state| {
        state.tokens.remove(device_id);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PersistedState {
        let mut device = Device::new("http://192.168.1.42:8080", "0123456789ab");
        device.id = "pixel-9".to_string();
        device.name = "Pixel 9".to_string();
        let mut state = PersistedState {
            last_device: Some(device),
            ..PersistedState::default()
        };
        state
            .usb_serials
            .insert("pixel-9".to_string(), "HT9AB0123".to_string());
        state
            .tokens
            .insert("pixel-9".to_string(), "0123456789ab".to_string());
        state.decorations.insert(
            "pixel-9".to_string(),
            HashMap::from([(
                "/storage/emulated/0/DCIM".to_string(),
                DecorationRecord {
                    pinned: true,
                    tag: Some(ColorTag::Slate),
                },
            )]),
        );
        state
    }

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("android18-state-{}-{name}", std::process::id()))
    }

    #[test]
    fn json_round_trip() {
        assert_eq!(PersistedState::from_json(&sample().to_json()), sample());
    }

    #[test]
    fn defaults_for_missing_fields_and_corrupt_files() {
        assert_eq!(PersistedState::from_json("{}"), PersistedState::default());
        assert_eq!(PersistedState::from_json("{{{"), PersistedState::default());
        // Fields from a newer build are ignored, known ones still load.
        let newer = PersistedState::from_json(r#"{"future":true,"usb_serials":{"a":"b"}}"#);
        assert_eq!(newer.usb_serials.get("a").map(String::as_str), Some("b"));
        assert_eq!(newer.last_device, None);
        assert!(newer.tokens.is_empty());
    }

    #[test]
    fn store_round_trips_through_the_file() {
        let path = temp("round-trip.json");
        let mut store = StateStore::open(Some(path.clone()));
        assert!(!store.persisted);
        store
            .update(|state| *state = sample())
            .expect("first write");
        assert!(store.persisted);
        assert_eq!(StateStore::open(Some(path.clone())).state, sample());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unchanged_updates_never_touch_disk() {
        let path = temp("no-op.json");
        let mut store = StateStore::open(Some(path.clone()));
        store.update(|state| *state = sample()).expect("write");
        // Corrupt the file; a same-value update must not rewrite it.
        std::fs::write(&path, "GARBAGE").expect("corrupt");
        store
            .update(|state| *state = sample())
            .expect("no-op update");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "GARBAGE");
        // A changed value does rewrite it.
        store
            .update(|state| state.last_device = None)
            .expect("changed update");
        assert_eq!(StateStore::open(Some(path.clone())).state.last_device, None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tokens_persist_overwrite_and_delete() {
        let path = temp("tokens.json");
        let mut store = StateStore::open(Some(path.clone()));
        store
            .update(|state| {
                state
                    .tokens
                    .insert("pixel-9".to_string(), "first".to_string());
            })
            .expect("save");
        store
            .update(|state| {
                state
                    .tokens
                    .insert("pixel-9".to_string(), "second".to_string());
            })
            .expect("overwrite");
        assert_eq!(
            StateStore::open(Some(path.clone()))
                .state
                .tokens
                .get("pixel-9")
                .map(String::as_str),
            Some("second")
        );
        store
            .update(|state| {
                state.tokens.remove("pixel-9");
            })
            .expect("delete");
        // Deleting an absent token is a no-op, not an error.
        store
            .update(|state| {
                state.tokens.remove("pixel-9");
            })
            .expect("absent delete");
        assert!(StateStore::open(Some(path.clone())).state.tokens.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn state_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp("perms.json");
        let mut store = StateStore::open(Some(path.clone()));
        store
            .update(|state| {
                state
                    .tokens
                    .insert("pixel-9".to_string(), "secret".to_string());
            })
            .expect("write");
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // A file some older build left group-readable is tightened on open.
        let mut perms = std::fs::metadata(&path).expect("stat").permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(&path, perms).expect("loosen");
        StateStore::open(Some(path.clone()));
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_home_is_in_memory_only() {
        let mut store = StateStore::open(None);
        assert!(!store.persisted);
        store
            .update(|state| *state = sample())
            .expect("memory write");
        assert!(!store.persisted);
        assert_eq!(store.state, sample());
    }
}
