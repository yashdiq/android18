//! View-state types shared by the browser UI: view mode and sorting.
//!
//! The sorting contract matches the phone API: directories always first,
//! then the chosen field, with a case-insensitive name tiebreak.

use std::cmp::Ordering;

use crate::domain::entry::Entry;
use crate::util::categorize::FileCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Table,
    Grid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Name,
    Size,
    Mtime,
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSpec {
    pub field: SortField,
    pub direction: SortDirection,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            field: SortField::Name,
            direction: SortDirection::Asc,
        }
    }
}

impl SortSpec {
    /// The same field with the direction flipped.
    pub fn toggled(self) -> Self {
        Self {
            field: self.field,
            direction: match self.direction {
                SortDirection::Asc => SortDirection::Desc,
                SortDirection::Desc => SortDirection::Asc,
            },
        }
    }
}

fn cmp_name(a: &Entry, b: &Entry) -> Ordering {
    a.name
        .to_lowercase()
        .cmp(&b.name.to_lowercase())
        .then_with(|| a.name.cmp(&b.name))
}

/// Sorts `entries` in place: directories first, then the spec's field
/// (respecting direction), then name ascending.
pub fn sort_entries(entries: &mut [Entry], spec: &SortSpec) {
    entries.sort_by(|a, b| compare_entries(a, b, spec));
}

/// Comparator used by [`sort_entries`]; exposed for keyboard/table headers.
pub fn compare_entries(a: &Entry, b: &Entry, spec: &SortSpec) -> Ordering {
    let field_cmp = match spec.field {
        SortField::Name => cmp_name(a, b),
        SortField::Size => a.size.cmp(&b.size).then_with(|| cmp_name(a, b)),
        SortField::Mtime => a.mtime.cmp(&b.mtime).then_with(|| cmp_name(a, b)),
        SortField::Type => FileCategory::of(a)
            .cmp(&FileCategory::of(b))
            .then_with(|| cmp_name(a, b)),
    };
    let field_cmp = match spec.direction {
        SortDirection::Asc => field_cmp,
        SortDirection::Desc => field_cmp.reverse(),
    };
    // Directories always precede files, regardless of sort direction.
    b.dir.cmp(&a.dir).then(field_cmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str, size: u64, mtime: i64) -> Entry {
        Entry::dir(&format!("/storage/emulated/0/{name}"), mtime, None, false).tap_size(size)
    }

    fn file(name: &str, size: u64, mtime: i64) -> Entry {
        Entry::file(
            &format!("/storage/emulated/0/{name}"),
            size,
            mtime,
            None,
            None,
        )
    }

    // Small helper so the constructors above can vary size for dirs too.
    trait TapSize {
        fn tap_size(self, size: u64) -> Self;
    }
    impl TapSize for Entry {
        fn tap_size(mut self, size: u64) -> Self {
            self.size = size;
            self
        }
    }

    #[test]
    fn directories_always_first() {
        let mut entries = vec![
            file("aaa.txt", 10, 0),
            dir("zzz", 1, 0),
            file("bbb.txt", 20, 0),
        ];
        sort_entries(&mut entries, &SortSpec::default());
        assert_eq!(entries[0].name, "zzz");
        assert!(entries[1..].iter().all(|e| !e.dir));
    }

    #[test]
    fn size_desc_keeps_directories_first() {
        let mut entries = vec![
            dir("small", 1, 0),
            file("huge.bin", 999, 0),
            file("tiny.txt", 2, 0),
        ];
        sort_entries(
            &mut entries,
            &SortSpec {
                field: SortField::Size,
                direction: SortDirection::Desc,
            },
        );
        assert_eq!(entries[0].name, "small");
        assert_eq!(entries[1].name, "huge.bin");
        assert_eq!(entries[2].name, "tiny.txt");
    }

    #[test]
    fn name_sort_is_case_insensitive() {
        let mut entries = vec![file("Beta", 0, 0), file("alpha", 0, 0)];
        sort_entries(&mut entries, &SortSpec::default());
        assert_eq!(entries[0].name, "alpha");
    }

    #[test]
    fn toggle_flips_direction() {
        let spec = SortSpec::default();
        assert_eq!(spec.toggled().direction, SortDirection::Desc);
    }
}
