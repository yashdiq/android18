//! Android18 domain core.
//!
//! Pure, GPUI-free domain logic for the Android18 desktop client:
//!
//! - [`domain`] — data types mirroring the phone-as-server JSON API
//!   ([`domain::Entry`], [`domain::Device`], [`domain::TransferItem`], …).
//! - [`fs`] — the Android storage-root model: canonical [`fs::paths`],
//!   the seeded [`fs::MockDevice`] backend, and reference seed data.
//! - [`port`] — the transport-agnostic [`port::DeviceBackend`] and
//!   [`port::SearchProvider`] contracts (ports & adapters).
//! - [`search`] — semantic file search, with an offline heuristic provider.
//! - [`shell`] — the engine behind the in-app terminal (REPL over a backend).
//! - [`transfer`] — the upload/download transfer state machine.
//! - [`util`] — formatting and file categorization helpers.
//!
//! Everything here is plain Rust: no UI framework, no async runtime lock-in,
//! fully unit-testable. The desktop app (GPUI Kit) and — later — the real
//! HTTP transport both build on these types.

pub mod domain;
pub mod fs;
pub mod port;
pub mod search;
pub mod shell;
pub mod transfer;
pub mod util;

/// The types used across crate boundaries.
pub mod prelude {
    pub use crate::domain::{
        ColorTag, Device, DeviceError, DeviceStatus, Entry, HttpLogEntry, HttpMethod,
        SortDirection, SortField, SortSpec, TransferDirection, TransferItem, TransferStatus,
        ViewMode,
    };
    pub use crate::fs::MockDevice;
    pub use crate::fs::paths::STORAGE_ROOT;
    pub use crate::port::{DeviceBackend, SearchProvider};
}
