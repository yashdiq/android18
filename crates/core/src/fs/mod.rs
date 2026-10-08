//! Storage model: the Android storage root, path canonicalization,
//! the seeded [`MockDevice`], and reference seed data.

pub mod mock;
pub mod paths;
pub mod seed;

pub use mock::MockDevice;
pub use paths::STORAGE_ROOT;
