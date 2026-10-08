use serde::{Deserialize, Serialize};

/// Folder color-tag palette, mirroring the web prototype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorTag {
    Blue,
    Emerald,
    Amber,
    Purple,
    Rose,
    Slate,
}

impl ColorTag {
    /// All tags in display order.
    pub const ALL: [ColorTag; ColorTag::COUNT] = [
        ColorTag::Blue,
        ColorTag::Emerald,
        ColorTag::Amber,
        ColorTag::Purple,
        ColorTag::Rose,
        ColorTag::Slate,
    ];

    /// Number of available tags (kept in sync with [`ColorTag::ALL`]).
    pub const COUNT: usize = 6;

    pub fn as_str(self) -> &'static str {
        match self {
            ColorTag::Blue => "blue",
            ColorTag::Emerald => "emerald",
            ColorTag::Amber => "amber",
            ColorTag::Purple => "purple",
            ColorTag::Rose => "rose",
            ColorTag::Slate => "slate",
        }
    }
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn extension_of(name: &str) -> Option<String> {
    name.rfind('.')
        .filter(|&i| i > 0 && i + 1 < name.len())
        .map(|i| name[i + 1..].to_lowercase())
}

/// A single filesystem entry, mirroring the phone's `GET /list` JSON schema.
///
/// The wire contract (ARCHITECTURE.md §"HTTP API") is the four mandatory
/// fields plus optional metadata; desktop-only decoration (pin, color tag)
/// is persisted locally and never sent to the phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub dir: bool,
    /// Bytes; `#[serde(default)]` because the phone's JSON encoder omits
    /// `size = 0` defaults (empty files) from `GET /list` responses.
    #[serde(default)]
    pub size: u64,
    /// Modification time, epoch milliseconds (also omitted when `0`).
    #[serde(default)]
    pub mtime: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    /// Direct child count for directories (computed by the backend on list).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_count: Option<u64>,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_tag: Option<ColorTag>,
    /// Text content for small text-ish files (preview + `cat`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl Entry {
    /// Creates a directory entry for `path`.
    pub fn dir(path: &str, mtime: i64, color_tag: Option<ColorTag>, is_pinned: bool) -> Self {
        Self {
            name: name_of(path).to_string(),
            path: path.to_string(),
            dir: true,
            size: 4096,
            mtime,
            mime_type: None,
            extension: None,
            item_count: None,
            is_pinned,
            color_tag,
            content: None,
        }
    }

    /// Creates a file entry for `path`, deriving name/extension from the path.
    pub fn file(
        path: &str,
        size: u64,
        mtime: i64,
        mime_type: Option<&str>,
        content: Option<String>,
    ) -> Self {
        let name = name_of(path).to_string();
        Self {
            extension: extension_of(&name),
            name,
            path: path.to_string(),
            dir: false,
            size,
            mtime,
            mime_type: mime_type.map(str::to_string),
            item_count: None,
            is_pinned: false,
            color_tag: None,
            content,
        }
    }

    /// Parent path (`None` only for the storage root itself).
    pub fn parent_path(&self) -> Option<&str> {
        self.path.rfind('/').map(|i| &self.path[..i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_entry_derives_name_from_path() {
        let e = Entry::dir("/storage/emulated/0/DCIM", 1_000, None, true);
        assert_eq!(e.name, "DCIM");
        assert!(e.dir);
        assert_eq!(e.size, 4096);
        assert_eq!(e.parent_path(), Some("/storage/emulated/0"));
    }

    #[test]
    fn file_entry_derives_name_and_extension() {
        let e = Entry::file(
            "/storage/emulated/0/notes/Readme.MD",
            12,
            1_000,
            Some("text/markdown"),
            None,
        );
        assert_eq!(e.name, "Readme.MD");
        assert_eq!(e.extension.as_deref(), Some("md")); // lowercased
        assert!(!e.dir);
    }

    #[test]
    fn dotfiles_have_no_extension() {
        let e = Entry::file("/storage/emulated/0/.hidden", 0, 0, None, None);
        assert_eq!(e.extension, None);
    }

    #[test]
    fn color_tags_round_trip_serde() {
        let tag: ColorTag = serde_json::from_str("\"amber\"")
            .ok()
            .unwrap_or_else(|| unreachable!());
        assert_eq!(tag, ColorTag::Amber);
    }

    #[test]
    fn parses_minimal_entry_json() {
        // The wire contract guarantees only name/path/dir; everything else
        // is default-valued in the phone's DTOs and may be stripped
        // (kotlinx `encodeDefaults = false`) or zero.
        let e: Entry =
            serde_json::from_str(r#"{"name":"a","path":"/storage/emulated/0/a","dir":false}"#)
                .expect("parse minimal entry");
        assert_eq!(e.name, "a");
        assert_eq!(e.size, 0);
        assert_eq!(e.mtime, 0);
        assert!(!e.is_pinned);
        assert_eq!(e.color_tag, None);
        assert_eq!(e.content, None);
    }
}
