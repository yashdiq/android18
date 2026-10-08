//! Domain data types: entries, devices, transfers, HTTP log lines, view
//! state (sort/view-mode), and the error taxonomy shared by all backends.

pub mod device;
pub mod entry;
pub mod error;
pub mod log;
pub mod transfer;
pub mod view;

pub use device::{Device, DeviceStatus, Transport};
pub use entry::{ColorTag, Entry};
pub use error::DeviceError;
pub use log::{HttpLogEntry, HttpMethod};
pub use transfer::{TransferDirection, TransferItem, TransferStatus};
pub use view::{SortDirection, SortField, SortSpec, ViewMode};
