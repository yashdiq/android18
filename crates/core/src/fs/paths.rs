//! Path canonicalization against the Android external-storage root.
//!
//! Ported from the web prototype's `resolve()` with one hardening change:
//! `..` segments are actually normalized, so a path that would escape the
//! storage root is rejected with `400` instead of merely failing a string
//! prefix check.

use crate::domain::error::DeviceError;

/// The only directory the phone ever exposes.
pub const STORAGE_ROOT: &str = "/storage/emulated/0";

/// Canonicalizes `path` against [`STORAGE_ROOT`].
///
/// - Relative paths are joined onto the storage root.
/// - Absolute paths outside the root are re-anchored onto it
///   (`/etc/passwd` → `/storage/emulated/0/etc/passwd`), matching the web
///   prototype.
/// - `.` and `..` segments are resolved; escaping the root is a
///   [`DeviceError::BadRequest`].
/// - Backslashes are treated as separators; duplicate separators collapse.
pub fn resolve(path: &str) -> Result<String, DeviceError> {
    let cleaned = path.trim().replace('\\', "/");
    let root_prefix = format!("{STORAGE_ROOT}/");

    // `~` expands to the storage root and a relative `0/…` repeats the root's
    // volume segment; both are collapsed (web-prototype parity).
    let cleaned = if cleaned == "~" {
        STORAGE_ROOT.to_string()
    } else if let Some(rest) = cleaned.strip_prefix("~/") {
        format!("{root_prefix}{rest}")
    } else if let Some(rest) = cleaned.strip_prefix("0/") {
        rest.to_string()
    } else {
        cleaned
    };

    let joined = if cleaned == STORAGE_ROOT || cleaned.starts_with(&root_prefix) {
        cleaned
    } else if cleaned.starts_with('/') {
        format!("{STORAGE_ROOT}{cleaned}")
    } else if cleaned.is_empty() {
        STORAGE_ROOT.to_string()
    } else {
        format!("{STORAGE_ROOT}/{cleaned}")
    };

    let suffix = joined.strip_prefix(STORAGE_ROOT).ok_or_else(|| {
        DeviceError::BadRequest("path traversal rejected: escaped storage root".into())
    })?;

    let mut stack: Vec<&str> = Vec::new();
    for segment in suffix.split('/').filter(|s| !s.is_empty()) {
        match segment {
            "." => {}
            ".." => {
                if stack.pop().is_none() {
                    return Err(DeviceError::BadRequest(
                        "path traversal rejected: escaped storage root".into(),
                    ));
                }
            }
            other => stack.push(other),
        }
    }

    if stack.is_empty() {
        Ok(STORAGE_ROOT.to_string())
    } else {
        Ok(format!("{}/{}", STORAGE_ROOT, stack.join("/")))
    }
}

/// Parent directory of `path` (`None` only for the storage root).
pub fn parent_of(path: &str) -> Option<String> {
    if path == STORAGE_ROOT {
        return None;
    }
    let idx = path.rfind('/')?;
    Some(if idx == 0 {
        "/".to_string()
    } else {
        path[..idx].to_string()
    })
}

/// True if `path` lies strictly below `ancestor` (a direct prefix on a
/// segment boundary).
pub fn is_descendant(path: &str, ancestor: &str) -> bool {
    path.len() > ancestor.len()
        && path.starts_with(ancestor)
        && path.as_bytes().get(ancestor.len()) == Some(&b'/')
}

/// Display form used across the UI and the shell prompt: the storage root
/// is shown as `~` (e.g. `~/DCIM/Camera`).
pub fn display_path(path: &str) -> String {
    path.replace(STORAGE_ROOT, "~")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_and_absolute() {
        assert_eq!(
            resolve("DCIM/Camera").unwrap(),
            format!("{STORAGE_ROOT}/DCIM/Camera")
        );
        assert_eq!(
            resolve("/etc/passwd").unwrap(),
            format!("{STORAGE_ROOT}/etc/passwd")
        );
        assert_eq!(resolve(STORAGE_ROOT).unwrap(), STORAGE_ROOT);
        assert_eq!(
            resolve("0//DCIM/./Camera").unwrap(),
            format!("{STORAGE_ROOT}/DCIM/Camera")
        );
        assert_eq!(resolve("   ").unwrap(), STORAGE_ROOT);
    }

    #[test]
    fn normalizes_dotdot() {
        assert_eq!(
            resolve("~/DCIM/../Download").unwrap(),
            format!("{STORAGE_ROOT}/Download")
        );
        assert_eq!(
            resolve(&format!("{STORAGE_ROOT}/DCIM/..")).unwrap(),
            STORAGE_ROOT
        );
    }

    #[test]
    fn rejects_escape_from_root() {
        assert!(matches!(
            resolve("../../etc/passwd"),
            Err(DeviceError::BadRequest(_))
        ));
        assert!(matches!(
            resolve(&format!("{STORAGE_ROOT}/../..")),
            Err(DeviceError::BadRequest(_))
        ));
    }

    #[test]
    fn parent_of_variants() {
        assert_eq!(parent_of(STORAGE_ROOT), None);
        assert_eq!(
            parent_of(&format!("{STORAGE_ROOT}/DCIM/Camera")),
            Some(format!("{STORAGE_ROOT}/DCIM"))
        );
    }

    #[test]
    fn descendant_checks() {
        let dcim = format!("{STORAGE_ROOT}/DCIM");
        assert!(is_descendant(&format!("{dcim}/Camera"), &dcim));
        assert!(!is_descendant(&format!("{STORAGE_ROOT}/DCIMX"), &dcim));
        assert!(!is_descendant(&dcim, &dcim));
    }

    #[test]
    fn display_path_replaces_root_with_tilde() {
        assert_eq!(display_path(STORAGE_ROOT), "~");
        assert_eq!(display_path(&format!("{STORAGE_ROOT}/DCIM")), "~/DCIM");
    }
}
