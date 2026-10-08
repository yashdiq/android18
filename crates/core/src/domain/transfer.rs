use serde::{Deserialize, Serialize};

/// Transfer direction relative to the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Upload,
    Download,
}

impl TransferDirection {
    pub fn label(self) -> &'static str {
        match self {
            TransferDirection::Upload => "upload",
            TransferDirection::Download => "download",
        }
    }
}

/// Lifecycle status of a queued transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    Queued,
    InProgress,
    Paused,
    Completed,
    Error,
    Cancelled,
}

impl TransferStatus {
    /// Whether the transfer still counts toward the "active" badge.
    pub fn is_active(self) -> bool {
        matches!(self, TransferStatus::Queued | TransferStatus::InProgress)
    }

    /// Whether no further transitions are possible.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TransferStatus::Completed | TransferStatus::Error | TransferStatus::Cancelled
        )
    }
}

/// One item in the transfer queue (upload or download).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransferItem {
    pub id: u64,
    pub name: String,
    pub direction: TransferDirection,
    pub size: u64,
    pub transferred: u64,
    pub status: TransferStatus,
    pub speed_bytes_per_sec: u64,
    pub remote_path: String,
    /// Range-resume supported (phone installs `PartialContent`).
    pub resumable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Enqueue time, epoch milliseconds.
    pub created_at: i64,
}

impl TransferItem {
    /// Progress as a fraction in `0.0..=1.0`.
    pub fn progress(&self) -> f32 {
        if self.size == 0 {
            if self.status == TransferStatus::Completed {
                1.0
            } else {
                0.0
            }
        } else {
            (self.transferred as f32 / self.size as f32).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(status: TransferStatus, transferred: u64, size: u64) -> TransferItem {
        TransferItem {
            id: 1,
            name: "IMG.jpg".into(),
            direction: TransferDirection::Download,
            size,
            transferred,
            status,
            speed_bytes_per_sec: 0,
            remote_path: "/storage/emulated/0/DCIM/IMG.jpg".into(),
            resumable: true,
            error_message: None,
            created_at: 0,
        }
    }

    #[test]
    fn progress_fraction() {
        assert!((item(TransferStatus::InProgress, 25, 100).progress() - 0.25).abs() < f32::EPSILON);
        assert_eq!(item(TransferStatus::InProgress, 250, 100).progress(), 1.0);
        assert_eq!(item(TransferStatus::Completed, 0, 0).progress(), 1.0);
    }

    #[test]
    fn status_flags() {
        assert!(TransferStatus::Queued.is_active());
        assert!(TransferStatus::InProgress.is_active());
        assert!(!TransferStatus::Paused.is_active());
        assert!(TransferStatus::Completed.is_terminal());
        assert!(TransferStatus::Cancelled.is_terminal());
        assert!(!TransferStatus::Paused.is_terminal());
    }
}
