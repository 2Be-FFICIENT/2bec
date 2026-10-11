//! Cursor-paginated list envelope shared by every `/v2` list endpoint.
//!
//! The API answers `{ "items": [...], "next": "<cursor>" | null, "total": n }`
//! and takes the cursor back as `?cursor=`. `limit` is 1..=200 per page.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::http::Client;

/// Largest page the API serves.
pub const MAX_LIMIT: u64 = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// How many items a list command should fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
    /// Up to `n` items, over as many pages as needed.
    Limit(u64),
    /// Every item, following cursors to the end.
    All,
}

/// Fetch items from `path`, following `next` cursors until `fetch` is
/// satisfied. Extra query parameters (filters) are repeated on every page.
pub async fn fetch_items<T: DeserializeOwned>(
    client: &Client,
    path: &str,
    filters: &[(&str, String)],
    fetch: Fetch,
) -> Result<Page<T>, ApiError> {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    let mut total = None;
    loop {
        let wanted = match fetch {
            Fetch::Limit(n) => n.saturating_sub(items.len() as u64),
            Fetch::All => MAX_LIMIT,
        };
        if wanted == 0 {
            return Ok(Page {
                items,
                next: cursor,
                total,
            });
        }
        let mut query: Vec<(&str, String)> = filters.to_vec();
        query.push(("limit", wanted.min(MAX_LIMIT).to_string()));
        if let Some(c) = &cursor {
            query.push(("cursor", c.clone()));
        }
        let page: Page<T> = client.get_with(path, &query).await?.json()?;
        total = page.total.or(total);
        items.extend(page.items);
        match page.next {
            Some(next) if !next.is_empty() => cursor = Some(next),
            _ => {
                return Ok(Page {
                    items,
                    next: None,
                    total,
                });
            }
        }
    }
}
