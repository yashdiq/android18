//! Transfer engine: a pure state machine over [`TransferItem`].
//!
//! The mock/backend layer reports progress ticks and lifecycle events; this
//! module decides which transitions are legal so the UI can never corrupt a
//! transfer's state.

mod engine;

pub use engine::{TransferError, TransferEvent, apply, new_transfer};
