use crate::metadata::musicbrainz::MusicBrainzClient;
use crate::metadata::types::{CanonicalRelease, CanonicalReleaseSummary, CanonicalTrack};
use crate::types::Platform;
use tauri::{command, AppHandle, Manager};

/// Searches MusicBrainz for canonical releases matching `query`.
#[command]
pub async fn search_canonical_releases(
    app: AppHandle,
    query: String,
) -> Result<Vec<CanonicalReleaseSummary>, String> {
    let cache_dir = app.path().app_cache_dir().ok();
    let client = MusicBrainzClient::new(cache_dir);
    let raw_summaries = client.search_releases(&query).await?;
    Ok(raw_summaries
        .into_iter()
        .map(|s| s.into_canonical_summary())
        .collect())
}

/// Fetches a full canonical release with complete tracklist and disc layout.
#[command]
pub async fn get_canonical_release(
    app: AppHandle,
    id: String,
) -> Result<CanonicalRelease, String> {
    let cache_dir = app.path().app_cache_dir().ok();
    let client = MusicBrainzClient::new(cache_dir);
    client.fetch_canonical_release(&id).await
}

/// Resolves audio candidates for a single canonical track across all platforms.
#[command]
pub async fn resolve_canonical_candidates(
    mut track: CanonicalTrack,
    fallback_artist: Option<String>,
) -> Result<CanonicalTrack, String> {
    crate::resolver::populate_track_candidates(&mut track, fallback_artist.as_deref()).await;
    Ok(track)
}

/// Prewarms stream resolution for tracks in the background to achieve near-instant preview playback.
#[command]
pub async fn prewarm_album_previews(
    app: AppHandle,
    items: Vec<(Platform, String)>,
) -> Result<(), String> {
    for (platform, url) in items {
        let app_clone = app.clone();
        tokio::spawn(async move {
            let _ = crate::preview::resolve::resolve(&app_clone, platform, &url).await;
        });
    }
    Ok(())
}
