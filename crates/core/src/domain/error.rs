/// Error taxonomy shared by every [`crate::port::DeviceBackend`].
///
/// Variants mirror the phone's HTTP status codes so the UI can map failures
/// to the exact UX states defined in ARCHITECTURE.md §"Error handling".
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DeviceError {
    #[error("400 Bad Request: {0}")]
    BadRequest(String),

    #[error("401 Unauthorized: invalid or missing X-Auth token")]
    Unauthorized,

    #[error("404 Not Found: {0}")]
    NotFound(String),

    #[error("409 Conflict: {0}")]
    Conflict(String),

    #[error("device offline: {0}")]
    Offline(String),

    #[error("i/o failure: {0}")]
    Io(String),
}

impl DeviceError {
    /// Equivalent HTTP status code (used when rendering request logs).
    pub fn status(&self) -> u16 {
        match self {
            DeviceError::BadRequest(_) => 400,
            DeviceError::Unauthorized => 401,
            DeviceError::NotFound(_) => 404,
            DeviceError::Conflict(_) => 409,
            DeviceError::Offline(_) => 503,
            DeviceError::Io(_) => 500,
        }
    }
}
