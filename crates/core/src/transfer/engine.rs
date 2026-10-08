//! Transition logic for the transfer queue.

use crate::domain::transfer::{TransferDirection, TransferItem, TransferStatus};

/// Lifecycle / progress event applied to a [`TransferItem`].
#[derive(Debug, Clone, PartialEq)]
pub enum TransferEvent {
    Start,
    Pause,
    Resume,
    Cancel,
    Complete,
    Fail {
        message: String,
    },
    Progress {
        delta_bytes: u64,
        speed_bytes_per_sec: u64,
    },
}

impl TransferEvent {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            TransferEvent::Start => "start",
            TransferEvent::Pause => "pause",
            TransferEvent::Resume => "resume",
            TransferEvent::Cancel => "cancel",
            TransferEvent::Complete => "complete",
            TransferEvent::Fail { .. } => "fail",
            TransferEvent::Progress { .. } => "progress",
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TransferError {
    #[error("invalid transition: cannot {event} a transfer that is {from:?}")]
    InvalidTransition {
        from: TransferStatus,
        event: &'static str,
    },
}

/// Creates a queued transfer.
pub fn new_transfer(
    id: u64,
    name: impl Into<String>,
    direction: TransferDirection,
    size: u64,
    remote_path: impl Into<String>,
    resumable: bool,
    now_ms: i64,
) -> TransferItem {
    TransferItem {
        id,
        name: name.into(),
        direction,
        size,
        transferred: 0,
        status: TransferStatus::Queued,
        speed_bytes_per_sec: 0,
        remote_path: remote_path.into(),
        resumable,
        error_message: None,
        created_at: now_ms,
    }
}

/// Applies `event` to `item` in place, rejecting illegal transitions.
pub fn apply(item: &mut TransferItem, event: TransferEvent) -> Result<(), TransferError> {
    let invalid = |item: &TransferItem, event: &'static str| TransferError::InvalidTransition {
        from: item.status,
        event,
    };
    let event_name = event.name();
    match event {
        TransferEvent::Start => {
            if item.status == TransferStatus::Queued {
                item.status = TransferStatus::InProgress;
                Ok(())
            } else {
                Err(invalid(item, event_name))
            }
        }
        TransferEvent::Pause => {
            if item.status == TransferStatus::InProgress {
                item.status = TransferStatus::Paused;
                item.speed_bytes_per_sec = 0;
                Ok(())
            } else {
                Err(invalid(item, event_name))
            }
        }
        TransferEvent::Resume => {
            if item.status == TransferStatus::Paused {
                item.status = TransferStatus::InProgress;
                Ok(())
            } else {
                Err(invalid(item, event_name))
            }
        }
        TransferEvent::Cancel => {
            if matches!(
                item.status,
                TransferStatus::Queued | TransferStatus::InProgress | TransferStatus::Paused
            ) {
                item.status = TransferStatus::Cancelled;
                item.speed_bytes_per_sec = 0;
                Ok(())
            } else {
                Err(invalid(item, event_name))
            }
        }
        TransferEvent::Complete => {
            if item.status == TransferStatus::InProgress {
                item.status = TransferStatus::Completed;
                item.transferred = item.size;
                item.speed_bytes_per_sec = 0;
                Ok(())
            } else {
                Err(invalid(item, event_name))
            }
        }
        TransferEvent::Fail { message } => {
            if item.status == TransferStatus::InProgress {
                item.status = TransferStatus::Error;
                item.error_message = Some(message);
                item.speed_bytes_per_sec = 0;
                Ok(())
            } else {
                Err(invalid(item, event_name))
            }
        }
        TransferEvent::Progress {
            delta_bytes,
            speed_bytes_per_sec,
        } => {
            if item.status == TransferStatus::InProgress {
                item.transferred = item.transferred.saturating_add(delta_bytes).min(item.size);
                item.speed_bytes_per_sec = speed_bytes_per_sec;
                Ok(())
            } else {
                Err(invalid(item, event_name))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued(size: u64) -> TransferItem {
        new_transfer(
            1,
            "backup.zip",
            TransferDirection::Upload,
            size,
            "~/Download/backup.zip",
            true,
            0,
        )
    }

    #[test]
    fn happy_path_progress_completes() {
        let mut t = queued(100);
        apply(&mut t, TransferEvent::Start).unwrap();
        apply(
            &mut t,
            TransferEvent::Progress {
                delta_bytes: 60,
                speed_bytes_per_sec: 10,
            },
        )
        .unwrap();
        assert_eq!(t.transferred, 60);
        assert_eq!(t.speed_bytes_per_sec, 10);
        apply(&mut t, TransferEvent::Complete).unwrap();
        assert_eq!(t.status, TransferStatus::Completed);
        assert_eq!(t.transferred, 100);
        assert!((t.progress() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pause_resume_cycle() {
        let mut t = queued(100);
        apply(&mut t, TransferEvent::Start).unwrap();
        apply(&mut t, TransferEvent::Pause).unwrap();
        assert_eq!(t.status, TransferStatus::Paused);
        assert_eq!(t.speed_bytes_per_sec, 0);
        apply(&mut t, TransferEvent::Resume).unwrap();
        assert_eq!(t.status, TransferStatus::InProgress);
    }

    #[test]
    fn progress_clamps_to_size() {
        let mut t = queued(50);
        apply(&mut t, TransferEvent::Start).unwrap();
        apply(
            &mut t,
            TransferEvent::Progress {
                delta_bytes: 999,
                speed_bytes_per_sec: 1,
            },
        )
        .unwrap();
        assert_eq!(t.transferred, 50);
    }

    #[test]
    fn illegal_transitions_rejected() {
        let mut t = queued(10);
        assert!(apply(&mut t, TransferEvent::Pause).is_err());
        assert!(apply(&mut t, TransferEvent::Resume).is_err());
        assert!(
            apply(
                &mut t,
                TransferEvent::Progress {
                    delta_bytes: 1,
                    speed_bytes_per_sec: 1
                }
            )
            .is_err()
        );
        apply(&mut t, TransferEvent::Start).unwrap();
        assert!(apply(&mut t, TransferEvent::Start).is_err());
        apply(&mut t, TransferEvent::Complete).unwrap();
        assert!(apply(&mut t, TransferEvent::Pause).is_err());
        assert!(apply(&mut t, TransferEvent::Cancel).is_err());
    }

    #[test]
    fn cancel_from_queued_and_paused() {
        let mut t = queued(10);
        apply(&mut t, TransferEvent::Cancel).unwrap();
        assert_eq!(t.status, TransferStatus::Cancelled);

        let mut t = queued(10);
        apply(&mut t, TransferEvent::Start).unwrap();
        apply(&mut t, TransferEvent::Pause).unwrap();
        apply(&mut t, TransferEvent::Cancel).unwrap();
        assert_eq!(t.status, TransferStatus::Cancelled);
    }

    #[test]
    fn fail_records_message() {
        let mut t = queued(10);
        apply(&mut t, TransferEvent::Start).unwrap();
        apply(
            &mut t,
            TransferEvent::Fail {
                message: "connection reset".into(),
            },
        )
        .unwrap();
        assert_eq!(t.status, TransferStatus::Error);
        assert_eq!(t.error_message.as_deref(), Some("connection reset"));
    }
}
