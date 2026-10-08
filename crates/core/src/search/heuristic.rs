//! Offline heuristic search, ported from the web prototype's server-side
//! fallback (`aiSearchService`). It interprets the natural-language query as
//! a conjunction of keyword categories (photos, audio, docs, code, large)
//! plus optional name/content substrings, and explains every match.

use async_trait::async_trait;

use crate::domain::entry::Entry;
use crate::fs::paths::display_path;
use crate::port::{Confidence, SearchError, SearchMatch, SearchProvider, SearchResult};

/// Size threshold for the "large files" category (matches the prototype).
const LARGE_FILE_THRESHOLD: u64 = 2_000_000;

const PHOTO_KEYWORDS: [&str; 3] = ["photo", "photos", "picture"];
const AUDIO_KEYWORDS: [&str; 3] = ["audio", "music", "song"];
const DOC_KEYWORDS: [&str; 2] = ["document", "documents"];
const CODE_KEYWORDS: [&str; 2] = ["code", "config"];
const LARGE_KEYWORDS: [&str; 1] = ["large"];

const IMAGE_EXTS: [&str; 5] = ["jpg", "jpeg", "png", "gif", "webp"];
const AUDIO_EXTS: [&str; 5] = ["mp3", "flac", "m4a", "wav", "aac"];
const DOC_EXTS: [&str; 3] = ["pdf", "doc", "docx"];
const CODE_EXTS: [&str; 6] = ["ts", "tsx", "json", "kt", "sh", "toml"];

fn extension_of(entry: &Entry) -> &str {
    entry.extension.as_deref().unwrap_or_default()
}

fn is_photo(e: &Entry) -> bool {
    IMAGE_EXTS.contains(&extension_of(e))
}

fn is_audio(e: &Entry) -> bool {
    AUDIO_EXTS.contains(&extension_of(e))
}

fn is_doc(e: &Entry) -> bool {
    DOC_EXTS.contains(&extension_of(e))
}

fn is_code(e: &Entry) -> bool {
    CODE_EXTS.contains(&extension_of(e))
}

fn is_large(e: &Entry) -> bool {
    !e.dir && e.size > LARGE_FILE_THRESHOLD
}

/// Deterministic keyword-heuristic search over an entry snapshot.
pub fn heuristic_search(query: &str, entries: &[Entry], current_path: &str) -> SearchResult {
    let q = query.to_lowercase();
    let wants = |keywords: &[&str]| keywords.iter().any(|k| q.contains(k));

    let wants_photo = wants(&PHOTO_KEYWORDS);
    let wants_audio = wants(&AUDIO_KEYWORDS);
    let wants_doc = wants(&DOC_KEYWORDS);
    let wants_code = wants(&CODE_KEYWORDS);
    let wants_large = wants(&LARGE_KEYWORDS);

    // Substring terms that are not category keywords.
    let keywords: Vec<&str> = PHOTO_KEYWORDS
        .iter()
        .chain(AUDIO_KEYWORDS.iter())
        .chain(DOC_KEYWORDS.iter())
        .chain(CODE_KEYWORDS.iter())
        .chain(LARGE_KEYWORDS.iter())
        .copied()
        .collect();
    // Generic filler words that never narrow a result.
    const STOPWORDS: [&str; 2] = ["file", "files"];
    let substrings: Vec<String> = q
        .split_whitespace()
        .filter(|w| !keywords.contains(w) && !STOPWORDS.contains(w) && w.len() >= 3)
        .map(str::to_string)
        .collect();
    let any_category = wants_photo || wants_audio || wants_doc || wants_code || wants_large;

    let mut matches: Vec<SearchMatch> = entries
        .iter()
        .filter_map(|e| {
            let name = e.name.to_lowercase();
            let path = e.path.to_lowercase();
            let content = e.content.as_deref().unwrap_or("").to_lowercase();
            let mut category_reasons: Vec<String> = Vec::new();

            if wants_photo && is_photo(e) {
                category_reasons.push("photo".into());
            }
            if wants_audio && is_audio(e) {
                category_reasons.push("audio".into());
            }
            if wants_doc && is_doc(e) {
                category_reasons.push("document".into());
            }
            if wants_code && is_code(e) {
                category_reasons.push("code/config".into());
            }
            if wants_large && is_large(e) {
                category_reasons.push(format!("large file (>{} bytes)", LARGE_FILE_THRESHOLD));
            }

            // The query is a conjunction: the entry must belong to a requested
            // category (when any was requested) and every substring term must
            // hit the name, content, or path.
            let category_ok = !any_category || !category_reasons.is_empty();
            let substrings_ok = substrings.iter().all(|term| {
                name.contains(term.as_str())
                    || content.contains(term.as_str())
                    || path.contains(term.as_str())
            });
            if !category_ok || !substrings_ok {
                return None;
            }

            let mut reasons = category_reasons;
            for term in &substrings {
                if name.contains(term.as_str()) {
                    reasons.push(format!("name contains \"{term}\""));
                } else if content.contains(term.as_str()) {
                    reasons.push(format!("content contains \"{term}\""));
                } else if path.contains(term.as_str()) {
                    reasons.push(format!("path contains \"{term}\""));
                }
            }
            if reasons.is_empty() {
                None
            } else {
                Some(SearchMatch {
                    path: display_path(&e.path),
                    reason: reasons.join(", "),
                    confidence: Confidence::Medium,
                })
            }
        })
        .collect();

    matches.sort_by(|a, b| a.path.cmp(&b.path));

    let summary = if matches.is_empty() {
        format!("No files matched \"{query}\" near {current_path}")
    } else {
        format!("Found {} matching file(s) for \"{query}\"", matches.len())
    };

    SearchResult {
        matches,
        summary,
        engine: Some("heuristic".into()),
        warning: None,
    }
}

/// [`SearchProvider`] adapter around [`heuristic_search`]; never fails.
pub struct HeuristicSearchProvider;

#[async_trait]
impl SearchProvider for HeuristicSearchProvider {
    async fn search(
        &self,
        query: &str,
        entries: &[Entry],
        current_path: &str,
    ) -> Result<SearchResult, SearchError> {
        Ok(heuristic_search(query, entries, current_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::seed::seed_entries;
    use futures::executor::block_on;

    const NOW: i64 = 1_772_000_000_000;

    #[test]
    fn photo_query_matches_images() {
        let entries: Vec<Entry> = seed_entries(NOW).into_iter().filter(|e| !e.dir).collect();
        let res = heuristic_search("camera photos", &entries, "~");
        assert!(res.summary.contains("Found"));
        assert!(
            res.matches
                .iter()
                .any(|m| m.path.ends_with("IMG_20261001_143022.jpg"))
        );
        assert!(res.matches.iter().all(|m| m.path.ends_with(".jpg")));
    }

    #[test]
    fn large_file_query_matches_over_threshold() {
        let entries: Vec<Entry> = seed_entries(NOW).into_iter().filter(|e| !e.dir).collect();
        let res = heuristic_search("large files", &entries, "~");
        assert!(res.matches.iter().all(|m| m.reason.contains("large")));
        assert!(
            !res.matches
                .iter()
                .any(|m| m.path.ends_with("system_log.txt"))
        );
    }

    #[test]
    fn content_substring_matches_notes() {
        let entries: Vec<Entry> = seed_entries(NOW).into_iter().filter(|e| !e.dir).collect();
        let res = heuristic_search("roadmap", &entries, "~");
        assert!(
            res.matches
                .iter()
                .any(|m| m.path.ends_with("Product_Roadmap_Q4.md"))
        );
    }

    #[test]
    fn empty_result_summarizes() {
        let entries: Vec<Entry> = seed_entries(NOW);
        let res = heuristic_search("xyzzy-plugh", &entries, "~");
        assert!(res.matches.is_empty());
        assert!(res.summary.contains("No files matched"));
    }

    #[test]
    fn provider_never_fails() {
        let provider = HeuristicSearchProvider;
        let entries = seed_entries(NOW);
        let res = block_on(provider.search("photos", &entries, "~")).unwrap();
        assert!(!res.matches.is_empty());
    }
}
