//! Real-device transport for Android18.
//!
//! The core crate defines the wire-shaped ports ([`DeviceBackend`],
//! [`SearchProvider`]) and an in-process mock. This crate supplies the real
//! adapters that talk to the Ktor service running on the phone:
//!
//! - [`HttpDevice`] — [`DeviceBackend`] over HTTP with `X-Auth`, the same
//!   400/401/404/409 error surface, an 80-entry request-log ring, and a
//!   resumable `Range` download helper for the transfer queue.
//! - [`HttpSearchProvider`] — [`SearchProvider`] posting `/search` to the
//!   phone (Gemini-backed there), falling back locally when offline.
//! - [`discovery`] — mDNS (`_android18._tcp`) browse plus `adb` USB devices.
//! - [`state`] — all desktop persistence (last device, USB serials,
//!   decorations, pairing tokens) in one owner-only JSON file; nothing
//!   sits behind a keychain ACL, so no launch flavor ever prompts.
//! - [`pairing`] — the one-shot QR pairing listener plus the
//!   phone-approved `/pair-request` connect flow.
//!
//! Every adapter uses a **blocking** reqwest client: they are only called
//! from plain background threads (the app spawns them via
//! `background_spawn`), never from inside an async runtime.

pub mod discovery;
pub mod http;
pub mod pairing;
pub mod search;
pub mod state;

pub use discovery::{
    AdbScan, DiscoveredDevice, DiscoveryReport, DiscoverySource, PHONE_PORT, adb_forward,
    adb_launch_connect, adb_scan, discover, discover_mdns, discover_usb,
};
pub use http::HttpDevice;
pub use pairing::{
    CODE_LEN, PairGrant, PairRequestError, PairingListener, PairingPayload, normalize_code,
    request_pairing,
};
pub use search::HttpSearchProvider;
pub use state::{
    DecorationRecord, clear_last_device, delete_token, load_decorations, load_last_device,
    load_token, load_usb_serial, save_decorations, save_last_device, save_token, save_usb_serial,
};
