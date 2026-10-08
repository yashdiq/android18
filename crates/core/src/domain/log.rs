use serde::{Deserialize, Serialize};

/// HTTP method of an emulated/observed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
        }
    }
}

/// One line of the live phone-server request log (Phone Server panel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpLogEntry {
    pub id: u64,
    /// Epoch milliseconds.
    pub timestamp: i64,
    pub method: HttpMethod,
    pub endpoint: String,
    pub status: u16,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}
