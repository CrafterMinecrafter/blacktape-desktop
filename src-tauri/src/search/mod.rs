use crate::search::bandcamp::ItemType;
use crate::types::Platform;
use serde::Serialize;
use std::collections::HashSet;
use std::error::Error;
use std::future::Future;
use std::time::Duration;

#[derive(Debug, Serialize, Clone)]
pub struct SearchSuggestion {
    pub item_type: ItemType,
    pub name: String,
    pub band_name: String,
    pub album_name: Option<String>,
    pub item_url_path: String,
    pub img: String,

    pub subscriber_count: Option<u64>,
    /// What `subscriber_count` counts: `"subscribers"` or `"monthly_audience"`; None when unknown/not applicable.
    pub audience_kind: Option<&'static str>,
    /// Page url of the album a TRACK belongs to (YouTube Music), so the UI can open the album from a track.
    pub album_url: Option<String>,
    pub view_count: Option<u64>,
    pub year: Option<i32>,
    pub duration: Option<u32>,
    pub platform: Platform,
}

pub mod bandcamp;
pub mod soundcloud;
pub mod bandcamp_full;
pub mod full;
pub mod ranking;
pub mod youtube;
pub mod youtube_fast;
pub mod youtube_full;
#[cfg(test)]
mod simulation_tests;

/// Time budget for the fast InnerTube path before the legacy rustypipe path takes over (D6).
const FAST_PATH_TIMEOUT: Duration = Duration::from_millis(3500);

type SearchResult = Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>>;

/// Fetches canonical album releases from MusicBrainz `release-group` API (under a strict timeout),
/// preferring primary studio albums and high-edition canonical entries, deduplicating `(artist, title)`
/// pairs, and converting them into `SearchSuggestion` items prefixed with `canonical:rg:<mbid>`.
pub async fn fetch_canonical_album_suggestions_result(
    query: &str,
    cache_dir: Option<std::path::PathBuf>,
    platform: Platform,
    limit: usize,
    timeout: Duration,
) -> Result<Vec<SearchSuggestion>, String> {
    let client = crate::metadata::MusicBrainzClient::new(cache_dir);
    let mut groups = match tokio::time::timeout(timeout, client.search_release_groups(query)).await {
        Ok(Ok(g)) => g,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            return Err("MusicBrainz request timed out after 6s. Please try again.".to_string());
        }
    };
    groups.retain(|rg| !crate::metadata::musicbrainz::is_bootleg_only_group(&rg.releases));
    // Prefer full studio Albums over EPs over Singles, no secondary types, and high release counts.
    groups.sort_by(|a, b| {
        let primary_rank = |p: Option<&str>| match p {
            Some(s) if s.eq_ignore_ascii_case("Album") => 2u8,
            Some(s) if s.eq_ignore_ascii_case("EP") => 1u8,
            _ => 0u8,
        };
        let a_primary = a.secondary_types.is_empty();
        let b_primary = b.secondary_types.is_empty();
        let a_type = primary_rank(a.primary_type.as_deref());
        let b_type = primary_rank(b.primary_type.as_deref());
        let a_weight = a.score.unwrap_or(0) + (a.count.unwrap_or(0).min(25) * 3);
        let b_weight = b.score.unwrap_or(0) + (b.count.unwrap_or(0).min(25) * 3);
        b_primary
            .cmp(&a_primary)
            .then_with(|| b_type.cmp(&a_type))
            .then_with(|| b_weight.cmp(&a_weight))
            .then_with(|| b.count.unwrap_or(0).cmp(&a.count.unwrap_or(0)))
    });
    let has_album_or_ep = groups.iter().any(|rg| {
        rg.primary_type
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("Album") || p.eq_ignore_ascii_case("EP"))
    });
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for rg in groups {
        let count = rg.count.unwrap_or(0);
        let is_single = rg
            .primary_type
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("Single"));
        // Singles with only 1 edition are tracks, not multi-source studio albums: skip them when
        // actual Albums/EPs exist so they never hijack top Album slots (e.g. "plastic death").
        if is_single && count < 2 && has_album_or_ep {
            continue;
        }
        let is_primary_studio = rg.secondary_types.is_empty()
            && rg
                .primary_type
                .as_deref()
                .map_or(true, |p| p.eq_ignore_ascii_case("Album") || p.eq_ignore_ascii_case("EP"));
        let summary = rg.into_canonical_summary();
        if summary.title.trim().is_empty() || summary.artist.trim().is_empty() {
            continue;
        }
        let key = (
            ranking::normalize(&summary.artist),
            ranking::normalize(&summary.title),
        );
        if !key.0.is_empty() && !key.1.is_empty() && !seen.insert(key) {
            continue;
        }
        // Encode edition prominence into internal view_count proxy (not displayed in UI) so
        // widely released studio albums (>= 3 editions) defeat 1-edition homonyms and compilations.
        let prominence_proxy = if count >= 3 && is_primary_studio {
            Some((count as u64).saturating_mul(25_000))
        } else if count >= 2 {
            Some((count as u64).saturating_mul(2_000))
        } else {
            None
        };
        deduped.push(SearchSuggestion {
            item_type: ItemType::Album,
            name: summary.title,
            band_name: summary.artist,
            album_name: None,
            item_url_path: format!("canonical:{}", summary.id),
            img: summary.cover_url.unwrap_or_default(),
            subscriber_count: None,
            audience_kind: None,
            album_url: None,
            view_count: prominence_proxy,
            year: summary.year.map(|y| y as i32),
            duration: None,
            platform,
        });
        if deduped.len() >= limit {
            break;
        }
    }
    Ok(deduped)
}

pub async fn fetch_canonical_album_suggestions(
    query: &str,
    cache_dir: Option<std::path::PathBuf>,
    platform: Platform,
    limit: usize,
    timeout: Duration,
) -> Vec<SearchSuggestion> {
    fetch_canonical_album_suggestions_result(query, cache_dir, platform, limit, timeout)
        .await
        .unwrap_or_default()
}

/// YouTube search with the fast InnerTube path first and the legacy rustypipe path as fallback (D6).
/// The fast path is given `FAST_PATH_TIMEOUT`; see `with_fallback` for the exact rules.
pub async fn search_youtube_with_fallback(query: &str) -> SearchResult {
    with_fallback(
        youtube_fast::search(query),
        youtube::search(query),
        FAST_PATH_TIMEOUT,
    )
    .await
}

/// Runs `fast` under `timeout`. Its result is returned as-is unless it is an error, a timeout, or
/// contains no Track/Album/Artist item (see `should_fallback`); then the reason is logged and `slow`
/// is awaited and its result returned unchanged. `slow` is not polled when `fast` succeeds.
async fn with_fallback<F, G>(fast: F, slow: G, timeout: Duration) -> SearchResult
where
    F: Future<Output = SearchResult>,
    G: Future<Output = SearchResult>,
{
    let reason = match tokio::time::timeout(timeout, fast).await {
        Ok(result) if !should_fallback(&result) => return result,
        Ok(Ok(_)) => "no track, album or artist results".to_string(),
        Ok(Err(e)) => format!("error: {e}"),
        Err(_) => format!("timeout after {} ms", timeout.as_millis()),
    };
    tracing::warn!("youtube_fast fallback: {reason}");
    slow.await
}

/// True when the fast path result is unusable: an error, or no item that is a Track, Album or Artist
/// (an empty list, or only `Unknown` items). Non-empty partial results are NOT a fallback case.
fn should_fallback(result: &SearchResult) -> bool {
    match result {
        Err(_) => true,
        Ok(items) => !items.iter().any(|item| {
            matches!(
                item.item_type,
                ItemType::Track | ItemType::Album | ItemType::Artist
            )
        }),
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn suggestion(item_type: ItemType, name: &str) -> SearchSuggestion {
        SearchSuggestion {
            item_type,
            name: name.to_string(),
            band_name: "Artist".to_string(),
            album_name: None,
            item_url_path: String::new(),
            img: String::new(),
            subscriber_count: None,
            audience_kind: None,
            album_url: None,
            view_count: None,
            year: None,
            duration: None,
            platform: Platform::Youtube,
        }
    }

    fn ok_items(items: Vec<SearchSuggestion>) -> SearchResult {
        Ok(items)
    }

    fn err(msg: &str) -> SearchResult {
        Err(msg.into())
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime")
    }

    #[test]
    fn should_fallback_on_error() {
        assert!(should_fallback(&err("network down")));
    }

    #[test]
    fn should_fallback_on_empty_list() {
        assert!(should_fallback(&ok_items(vec![])));
    }

    #[test]
    fn should_fallback_when_only_unknown_items() {
        let items = vec![
            suggestion(ItemType::Unknown, "Video"),
            suggestion(ItemType::Unknown, "Playlist"),
        ];
        assert!(should_fallback(&ok_items(items)));
    }

    #[test]
    fn should_not_fallback_on_track_album_or_artist() {
        for t in [ItemType::Track, ItemType::Album, ItemType::Artist] {
            assert!(!should_fallback(&ok_items(vec![suggestion(t, "x")])));
        }
    }

    #[test]
    fn should_not_fallback_when_unknown_mixed_with_track() {
        let items = vec![
            suggestion(ItemType::Unknown, "Video"),
            suggestion(ItemType::Track, "One More Time"),
        ];
        assert!(!should_fallback(&ok_items(items)));
    }

    #[test]
    fn should_fallback_keeps_unicode_names_irrelevant() {
        // Name content must not influence the decision; only the item type does.
        assert!(should_fallback(&ok_items(vec![suggestion(
            ItemType::Unknown,
            "Сигур Рос 日本語 🎵"
        )])));
        assert!(!should_fallback(&ok_items(vec![suggestion(
            ItemType::Track,
            "Сигур Рос 日本語 🎵"
        )])));
    }

    #[test]
    fn fast_success_is_returned_and_fallback_not_polled() {
        let rt = runtime();
        let slow_polled = Arc::new(AtomicBool::new(false));
        let flag = slow_polled.clone();
        let result = rt.block_on(with_fallback(
            async { ok_items(vec![suggestion(ItemType::Track, "Fast")]) },
            async move {
                flag.store(true, Ordering::SeqCst);
                ok_items(vec![suggestion(ItemType::Track, "Slow")])
            },
            Duration::from_secs(1),
        ));
        let items = result.expect("fast result");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Fast");
        assert!(!slow_polled.load(Ordering::SeqCst));
    }

    #[test]
    fn fast_error_falls_back_to_slow() {
        let rt = runtime();
        let items = rt
            .block_on(with_fallback(
                async { err("fast broke") },
                async { ok_items(vec![suggestion(ItemType::Album, "Legacy")]) },
                Duration::from_secs(1),
            ))
            .expect("fallback result");
        assert_eq!(items[0].name, "Legacy");
    }

    #[test]
    fn fast_empty_falls_back_to_slow() {
        let rt = runtime();
        let items = rt
            .block_on(with_fallback(
                async { ok_items(vec![]) },
                async { ok_items(vec![suggestion(ItemType::Artist, "Legacy")]) },
                Duration::from_secs(1),
            ))
            .expect("fallback result");
        assert_eq!(items[0].name, "Legacy");
    }

    #[test]
    fn fast_timeout_falls_back_and_cancels_fast() {
        let rt = runtime();
        let started = std::time::Instant::now();
        let items = rt
            .block_on(with_fallback(
                async {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    ok_items(vec![suggestion(ItemType::Track, "Too late")])
                },
                async { ok_items(vec![suggestion(ItemType::Track, "Legacy")]) },
                Duration::from_millis(20),
            ))
            .expect("fallback result");
        assert_eq!(items[0].name, "Legacy");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn fast_partial_result_within_timeout_is_kept() {
        let rt = runtime();
        let items = rt
            .block_on(with_fallback(
                async {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    ok_items(vec![suggestion(ItemType::Track, "Partial")])
                },
                async { ok_items(vec![suggestion(ItemType::Track, "Legacy")]) },
                Duration::from_secs(2),
            ))
            .expect("fast result");
        assert_eq!(items[0].name, "Partial");
    }

    #[test]
    fn fallback_error_is_returned_unchanged() {
        let rt = runtime();
        let result = rt.block_on(with_fallback(
            async { err("fast broke") },
            async { err("legacy broke") },
            Duration::from_secs(1),
        ));
        let msg = result.err().expect("error").to_string();
        assert_eq!(msg, "legacy broke");
    }

    #[test]
    fn fast_error_with_empty_fallback_returns_empty_ok() {
        let rt = runtime();
        let result = rt.block_on(with_fallback(
            async { err("fast broke") },
            async { ok_items(vec![]) },
            Duration::from_secs(1),
        ));
        assert!(result.expect("ok").is_empty());
    }
}
