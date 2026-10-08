//! File-type categorization for icons, badges, and sort-by-type,
//! ported from the web prototype's `getFileCategory`.

use crate::domain::entry::Entry;

/// Coarse file type. Variant order defines the "sort by type" ordering:
/// folders first, then media, documents, and finally everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileCategory {
    Folder,
    Image,
    Video,
    Audio,
    Document,
    MarkdownNote,
    Spreadsheet,
    Archive,
    AndroidPackage,
    BinaryImage,
    CodeConfig,
    File,
}

impl FileCategory {
    /// Categorizes an entry (directories are always [`FileCategory::Folder`]).
    pub fn of(entry: &Entry) -> Self {
        if entry.dir {
            return Self::Folder;
        }
        let ext = entry.extension.as_deref().unwrap_or_default();
        Self::from_extension(ext)
    }

    /// Categorizes a lowercase file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "bmp" | "heic" => Self::Image,
            "mp4" | "mkv" | "mov" | "webm" | "avi" | "3gp" => Self::Video,
            "mp3" | "flac" | "m4a" | "wav" | "aac" | "ogg" | "opus" => Self::Audio,
            "md" | "markdown" => Self::MarkdownNote,
            "xls" | "xlsx" | "csv" => Self::Spreadsheet,
            "pdf" | "doc" | "docx" | "txt" | "rtf" => Self::Document,
            "zip" | "rar" | "7z" | "tar" | "gz" => Self::Archive,
            "apk" => Self::AndroidPackage,
            "bin" | "iso" | "img" | "dat" => Self::BinaryImage,
            "ts" | "tsx" | "js" | "json" | "kt" | "rs" | "py" | "sh" | "toml" | "yaml" | "yml" => {
                Self::CodeConfig
            }
            _ => Self::File,
        }
    }

    /// Display label (matches the web prototype's type badges).
    pub fn label(self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::Image => "Image",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Document => "Document",
            Self::MarkdownNote => "Markdown Note",
            Self::Spreadsheet => "Spreadsheet",
            Self::Archive => "Archive",
            Self::AndroidPackage => "APK",
            Self::BinaryImage => "Binary Image",
            Self::CodeConfig => "Code/Config",
            Self::File => "File",
        }
    }
}

impl std::fmt::Display for FileCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_with_ext(ext: &str) -> Entry {
        let name = if ext.is_empty() {
            "noext".to_string()
        } else {
            format!("x.{ext}")
        };
        Entry::file(&format!("/storage/emulated/0/{name}"), 1, 0, None, None)
    }

    #[test]
    fn categorizes_extensions() {
        assert_eq!(FileCategory::of(&file_with_ext("jpg")), FileCategory::Image);
        assert_eq!(
            FileCategory::of(&file_with_ext("md")),
            FileCategory::MarkdownNote
        );
        assert_eq!(
            FileCategory::of(&file_with_ext("apk")),
            FileCategory::AndroidPackage
        );
        assert_eq!(
            FileCategory::of(&file_with_ext("bin")),
            FileCategory::BinaryImage
        );
        assert_eq!(
            FileCategory::of(&file_with_ext("csv")),
            FileCategory::Spreadsheet
        );
        assert_eq!(FileCategory::of(&file_with_ext("zzz")), FileCategory::File);
        assert_eq!(FileCategory::of(&file_with_ext("")), FileCategory::File);
    }

    #[test]
    fn directories_are_folders() {
        let d = Entry::dir("/storage/emulated/0/DCIM", 0, None, false);
        assert_eq!(FileCategory::of(&d), FileCategory::Folder);
    }

    #[test]
    fn folder_sorts_before_everything() {
        assert!(FileCategory::Folder < FileCategory::Image);
        assert!(FileCategory::Image < FileCategory::Document);
        assert!(FileCategory::File > FileCategory::CodeConfig);
    }
}
