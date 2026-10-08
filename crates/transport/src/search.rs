//! [`SearchProvider`] backed by the phone service `POST /search` endpoint
//! (GLM via Z.AI there, with a device-side heuristic fallback).

use android18_core::fs::paths::{STORAGE_ROOT, display_path};
use android18_core::port::{Confidence, SearchError, SearchMatch, SearchProvider, SearchResult};
use android18_core::search::HeuristicSearchProvider;
use async_trait::async_trait;
use serde::Deserialize;

/// Wire shape of one match: `{"path","reason","confidence"}`.
#[derive(Debug, Deserialize)]
struct SearchMatchDto {
    path: String,
    reason: String,
    confidence: Confidence,
}

/// Wire shape of the response: `{"summary","matches":[…]}`.
#[derive(Debug, Deserialize)]
struct SearchResponseDto {
    summary: String,
    #[serde(default)]
    matches: Vec<SearchMatchDto>,
    #[serde(default)]
    engine: Option<String>,
    #[serde(default)]
    warning: Option<String>,
}

/// Search provider that defers to the phone. Falls back to the local
/// heuristic when the device is unreachable so ⌘K always answers.
pub struct HttpSearchProvider {
    client: reqwest::blocking::Client,
    base_url: String,
    token: String,
}

impl HttpSearchProvider {
    /// Builds the provider; `base_url` is `http://ip:port` (no trailing `/`).
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url: base_url.into(),
            token: token.into(),
        }
    }
}

impl HttpSearchProvider {
    fn remote_search(&self, query: &str, current_path: &str) -> Result<SearchResult, SearchError> {
        let url = format!("{}/search", self.base_url);
        let body = serde_json::json!({ "query": query, "path": current_path });
        let response = self
            .client
            .post(url)
            .header("X-Auth", &self.token)
            .json(&body)
            .send()
            .map_err(|e| SearchError::Unavailable(e.to_string()))?;
        let dto: SearchResponseDto = response
            .error_for_status()
            .map_err(|e| SearchError::Unavailable(e.to_string()))?
            .json()
            .map_err(|e| SearchError::Unavailable(e.to_string()))?;
        // The phone fell back to its own heuristic (no key, or the AI call
        // failed): surface why instead of passing it off as AI.
        let warning = dto.warning.filter(|w| !w.is_empty());
        Ok(SearchResult {
            summary: dto.summary,
            engine: dto.engine,
            warning,
            matches: dto
                .matches
                .into_iter()
                .map(|m| {
                    let mut path = m.path;
                    if path.starts_with(STORAGE_ROOT) {
                        path = display_path(&path);
                    }
                    SearchMatch {
                        path,
                        reason: m.reason,
                        confidence: m.confidence,
                    }
                })
                .collect(),
        })
    }
}

#[async_trait]
impl SearchProvider for HttpSearchProvider {
    async fn search(
        &self,
        query: &str,
        entries: &[android18_core::domain::Entry],
        current_path: &str,
    ) -> Result<SearchResult, SearchError> {
        let attempt = self.remote_search(query, current_path);
        match attempt {
            Ok(result) => Ok(result),
            // Offline/unauthorized phone: answer locally, but say so.
            Err(error) => {
                let mut result = HeuristicSearchProvider
                    .search(query, entries, current_path)
                    .await?;
                result.warning = Some(format!("AI unavailable ({error}); showing keyword results"));
                Ok(result)
            }
        }
    }
}
