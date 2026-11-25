use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};
use crate::resolver::matcher::{
    evaluate_artist_match, evaluate_track_match, matches_album_listing,
    strip_leading_artist_from_title, ArtistMatch,
};
use crate::search::bandcamp::ItemType;
use tracing::{debug, warn};

const MAX_FALLBACK_CANDIDATES_PER_PLATFORM: usize = 2;

fn accept_track_candidate(
    artist: &str,
    title: &str,
    duration_ms: u64,
    cand_name: &str,
    cand_band_name: &str,
    cand_duration_ms: u64,
) -> Option<(f32, bool)> {
    let artist_match = evaluate_artist_match(artist, cand_band_name, cand_name);
    if artist_match == ArtistMatch::Mismatch {
        return None;
    }

    let stripped_name = strip_leading_artist_from_title(artist, cand_name);
    let raw_eval = evaluate_track_match(title, duration_ms, cand_name, cand_duration_ms);
    let stripped_eval = evaluate_track_match(title, duration_ms, &stripped_name, cand_duration_ms);
    let eval = if stripped_eval.confidence > raw_eval.confidence {
        stripped_eval
    } else {
        raw_eval
    };

    if eval.has_version_mismatch || eval.has_duration_mismatch {
        return None;
    }
    if artist_match == ArtistMatch::Neutral && (!eval.is_strict_match || eval.title_similarity < 0.90)
    {
        return None;
    }
    if eval.title_similarity < 0.78 || eval.confidence < 0.65 {
        return None;
    }

    let boost = if artist_match == ArtistMatch::Match { 0.04 } else { 0.0 };
    let confidence = (eval.confidence + boost).clamp(0.0, 1.0);
    let is_strict = eval.is_strict_match && artist_match == ArtistMatch::Match;
    Some((confidence, is_strict))
}

fn keep_top_candidates(mut candidates: Vec<AudioCandidate>) -> Vec<AudioCandidate> {
    candidates.sort_by(|a, b| {
        b.is_strict_match.cmp(&a.is_strict_match).then_with(|| {
            b.match_confidence
                .partial_cmp(&a.match_confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    candidates.truncate(MAX_FALLBACK_CANDIDATES_PER_PLATFORM);
    candidates
}

/// Queries YouTube for candidate streams matching the target track.
pub async fn resolve_youtube_candidates(
    artist: &str,
    title: &str,
    duration_ms: u64,
) -> Vec<AudioCandidate> {
    let query = format!("{} {}", artist, title);
    let search_res = crate::search::search_youtube_with_fallback(&query).await;

    let suggestions = match search_res {
        Ok(s) => s,
        Err(e) => {
            warn!("YouTube candidate search failed for '{}': {}", query, e);
            return Vec::new();
        }
    };

    let mut candidates = Vec::new();
    for item in suggestions {
        if item.item_type != ItemType::Track {
            continue;
        }

        let cand_duration_ms = item.duration.map(|d| d as u64 * 1000).unwrap_or(0);
        if let Some((confidence, is_strict_match)) = accept_track_candidate(
            artist,
            title,
            duration_ms,
            &item.name,
            &item.band_name,
            cand_duration_ms,
        ) {
            let matched_artist = (!item.band_name.trim().is_empty()).then(|| item.band_name.clone());
            candidates.push(AudioCandidate {
                source: AudioSource::HttpStream {
                    platform: "youtube".into(),
                    url: item.item_url_path,
                    format_id: None,
                    direct_stream_url: None,
                    expires_at: None,
                },
                quality_tier: AudioQualityTier::HighLossy,
                codec: "Opus".into(),
                bitrate_kbps: Some(160),
                sample_rate_hz: Some(48000),
                duration_ms: cand_duration_ms,
                match_confidence: confidence,
                is_strict_match,
                display_label: "YouTube (Opus 160k)".into(),
                matched_title: Some(item.name),
                matched_artist,
            });
        }
    }

    let top = keep_top_candidates(candidates);
    debug!(
        "Resolved {} YouTube candidate(s) for '{} - {}'",
        top.len(),
        artist,
        title
    );
    top
}

/// Queries Bandcamp for candidate streams matching the target track.
pub async fn resolve_bandcamp_candidates(
    artist: &str,
    title: &str,
    duration_ms: u64,
) -> Vec<AudioCandidate> {
    let query = format!("{} {}", artist, title);
    let search_res = crate::search::bandcamp::search(&query).await;

    let suggestions = match search_res {
        Ok(s) => s,
        Err(e) => {
            warn!("Bandcamp candidate search failed for '{}': {}", query, e);
            return Vec::new();
        }
    };

    let mut candidates = Vec::new();
    for item in suggestions {
        if item.item_type != ItemType::Track {
            continue;
        }

        let cand_duration_ms = item.duration.map(|d| d as u64 * 1000).unwrap_or(0);
        if let Some((confidence, is_strict_match)) = accept_track_candidate(
            artist,
            title,
            duration_ms,
            &item.name,
            &item.band_name,
            cand_duration_ms,
        ) {
            let matched_artist = (!item.band_name.trim().is_empty()).then(|| item.band_name.clone());
            candidates.push(AudioCandidate {
                source: AudioSource::HttpStream {
                    platform: "bandcamp".into(),
                    url: item.item_url_path,
                    format_id: None,
                    direct_stream_url: None,
                    expires_at: None,
                },
                quality_tier: AudioQualityTier::StandardLossy,
                codec: "MP3".into(),
                bitrate_kbps: Some(128),
                sample_rate_hz: Some(44100),
                duration_ms: cand_duration_ms,
                match_confidence: confidence,
                is_strict_match,
                display_label: "Bandcamp (MP3 128k)".into(),
                matched_title: Some(item.name),
                matched_artist,
            });
        }
    }

    let mut top = keep_top_candidates(candidates);
    if !top.is_empty() {
        let free_url = top.first().map(|c| c.source.primary_url().to_string());
        if let Some(u) = free_url {
            if crate::download::bandcamp::page_has_free_download(&u).await {
                for cand in &mut top {
                    cand.quality_tier = AudioQualityTier::Lossless;
                    cand.codec = "FLAC".into();
                    cand.bitrate_kbps = None;
                    cand.sample_rate_hz = Some(44100);
                    cand.display_label = "Bandcamp · FLAC (Lossless)".into();
                }
            }
        }
    }
    debug!(
        "Resolved {} Bandcamp candidate(s) for '{} - {}'",
        top.len(),
        artist,
        title
    );
    top
}

/// Queries SoundCloud for candidate streams matching the target track.
pub async fn resolve_soundcloud_candidates(
    artist: &str,
    title: &str,
    duration_ms: u64,
) -> Vec<AudioCandidate> {
    let query = format!("{} {}", artist, title);
    let search_res = crate::search::soundcloud::search(&query).await;

    let suggestions = match search_res {
        Ok(s) => s,
        Err(e) => {
            warn!("SoundCloud candidate search failed for '{}': {}", query, e);
            return Vec::new();
        }
    };

    let mut candidates = Vec::new();
    for item in suggestions {
        if item.item_type != ItemType::Track {
            continue;
        }

        let cand_duration_ms = item.duration.map(|d| d as u64 * 1000).unwrap_or(0);
        if let Some((confidence, is_strict_match)) = accept_track_candidate(
            artist,
            title,
            duration_ms,
            &item.name,
            &item.band_name,
            cand_duration_ms,
        ) {
            let matched_artist = (!item.band_name.trim().is_empty()).then(|| item.band_name.clone());
            candidates.push(AudioCandidate {
                source: AudioSource::HttpStream {
                    platform: "soundcloud".into(),
                    url: item.item_url_path,
                    format_id: None,
                    direct_stream_url: None,
                    expires_at: None,
                },
                quality_tier: AudioQualityTier::StandardLossy,
                codec: "MP3".into(),
                bitrate_kbps: Some(128),
                sample_rate_hz: Some(44100),
                duration_ms: cand_duration_ms,
                match_confidence: confidence,
                is_strict_match,
                display_label: "SoundCloud (MP3 128k)".into(),
                matched_title: Some(item.name),
                matched_artist,
            });
        }
    }

    let top = keep_top_candidates(candidates);
    debug!(
        "Resolved {} SoundCloud candidate(s) for '{} - {}'",
        top.len(),
        artist,
        title
    );
    top
}

/// Placeholder for BitTorrent swarm provider (Milestone 2).
/// Returns an empty list in Milestone 1, fully compliant with the polymorphic `AudioSource::Torrent` schema.
pub async fn resolve_torrent_candidates(
    _artist: &str,
    _title: &str,
    _duration_ms: u64,
) -> Vec<AudioCandidate> {
    Vec::new()
}

/// Queries YouTube Music for the full album matching `artist` and `album_title`.
/// In a single request, returns all tracks of the official album release.
pub async fn resolve_youtube_album_tracks(
    artist: &str,
    album_title: &str,
) -> Vec<crate::download::TrackDownload> {
    let query = format!("{} {}", artist, album_title);
    let search_res = crate::search::youtube_fast::search(&query).await;
    let suggestions = match search_res {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    for item in suggestions {
        if item.item_type == ItemType::Album
            && item.item_url_path.contains("browse/MPREb_")
            && matches_album_listing(artist, album_title, &item.name, &item.band_name)
        {
            if let Ok(album_download) = crate::download::youtube::parse_album(&item.item_url_path).await {
                debug!(
                    "Resolved {} tracks from YouTube Music album '{}'",
                    album_download.tracks.len(),
                    item.item_url_path
                );
                return album_download.tracks;
            }
        }
    }
    Vec::new()
}

/// Queries Bandcamp for the full album matching `artist` and `album_title`, along with whether it offers free lossless downloads.
pub async fn resolve_bandcamp_album_tracks(
    artist: &str,
    album_title: &str,
) -> (Vec<crate::download::TrackDownload>, bool) {
    let query = format!("{} {}", artist, album_title);
    let search_res = crate::search::bandcamp::search(&query).await;
    let suggestions = match search_res {
        Ok(s) => s,
        Err(_) => return (Vec::new(), false),
    };

    for item in suggestions {
        if item.item_type == ItemType::Album
            && item.item_url_path.contains("/album/")
            && matches_album_listing(artist, album_title, &item.name, &item.band_name)
        {
            if let Ok((album_download, is_free_download)) =
                crate::download::bandcamp::parse_album_with_free_flag(&item.item_url_path).await
            {
                debug!(
                    "Resolved {} tracks from Bandcamp album '{}' (free_lossless={})",
                    album_download.tracks.len(),
                    item.item_url_path,
                    is_free_download
                );
                return (album_download.tracks, is_free_download);
            }
        }
    }
    (Vec::new(), false)
}

/// Queries SoundCloud for the full album / set matching `artist` and `album_title`.
pub async fn resolve_soundcloud_album_tracks(
    artist: &str,
    album_title: &str,
) -> Vec<crate::download::TrackDownload> {
    let query = format!("{} {}", artist, album_title);
    let search_res = crate::search::soundcloud::search(&query).await;
    let suggestions = match search_res {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    for item in suggestions {
        if item.item_type == ItemType::Album
            && item.item_url_path.contains("/sets/")
            && matches_album_listing(artist, album_title, &item.name, &item.band_name)
        {
            if let Ok((album_download, _)) = crate::download::soundcloud::parse_album_with_skipped(&item.item_url_path).await {
                debug!(
                    "Resolved {} tracks from SoundCloud album '{}'",
                    album_download.tracks.len(),
                    item.item_url_path
                );
                return album_download.tracks;
            }
        }
    }
    Vec::new()
}

/// Searches YouTube Music, Bandcamp, and SoundCloud concurrently for matching album releases
/// to display alongside MusicBrainz editions in the AlbumPreview release picker.
pub async fn find_platform_album_editions(
    artist: &str,
    album_title: &str,
) -> Vec<crate::metadata::types::ReleaseEditionOption> {
    let artist = artist.trim();
    let album_title = album_title.trim();
    if artist.is_empty() || album_title.is_empty() {
        return Vec::new();
    }
    let query = format!("{artist} {album_title}");
    let timeout = std::time::Duration::from_millis(2200);

    let yt_fut = tokio::time::timeout(timeout, crate::search::youtube_fast::search(&query));
    let bc_fut = tokio::time::timeout(timeout, crate::search::bandcamp::search(&query));
    let sc_fut = tokio::time::timeout(timeout, crate::search::soundcloud::search(&query));

    let (yt_res, bc_res, sc_res) = tokio::join!(yt_fut, bc_fut, sc_fut);

    let mut out = Vec::new();

    if let Ok(Ok(items)) = yt_res {
        if let Some(item) = items.into_iter().find(|item| {
            item.item_type == ItemType::Album
                && (item.item_url_path.contains("browse/MPREb_")
                    || item.item_url_path.contains("list=OLAK"))
                && matches_album_listing(artist, album_title, &item.name, &item.band_name)
        }) {
            out.push(crate::metadata::types::ReleaseEditionOption {
                id: format!("yt:{}", item.item_url_path),
                title: item.name,
                edition_label: "YouTube Music".to_string(),
                year: item.year,
                track_count: 0,
                disc_count: 1,
                format: Some("Opus High".to_string()),
                is_default: false,
                is_alternate: false,
                source_kind: Some("youtube".to_string()),
                url: Some(item.item_url_path),
            });
        }
    }

    if let Ok(Ok(items)) = bc_res {
        if let Some(item) = items.into_iter().find(|item| {
            item.item_type == ItemType::Album
                && item.item_url_path.contains("/album/")
                && matches_album_listing(artist, album_title, &item.name, &item.band_name)
        }) {
            out.push(crate::metadata::types::ReleaseEditionOption {
                id: format!("bc:{}", item.item_url_path),
                title: item.name,
                edition_label: "Bandcamp".to_string(),
                year: item.year,
                track_count: 0,
                disc_count: 1,
                format: Some("Bandcamp".to_string()),
                is_default: false,
                is_alternate: false,
                source_kind: Some("bandcamp".to_string()),
                url: Some(item.item_url_path),
            });
        }
    }

    if let Ok(Ok(items)) = sc_res {
        if let Some(item) = items.into_iter().find(|item| {
            item.item_type == ItemType::Album
                && item.item_url_path.contains("/sets/")
                && matches_album_listing(artist, album_title, &item.name, &item.band_name)
        }) {
            out.push(crate::metadata::types::ReleaseEditionOption {
                id: format!("sc:{}", item.item_url_path),
                title: item.name,
                edition_label: "SoundCloud".to_string(),
                year: item.year,
                track_count: 0,
                disc_count: 1,
                format: Some("SoundCloud".to_string()),
                is_default: false,
                is_alternate: false,
                source_kind: Some("soundcloud".to_string()),
                url: Some(item.item_url_path),
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_track_candidate_rejects_wrong_artist_and_unrelated_titles() {
        assert_eq!(
            accept_track_candidate("Glass Beach", "Cul-de-sac", 225_000, "Cul-de-sac", "Genesis", 225_000),
            None
        );
        assert_eq!(
            accept_track_candidate("Glass Beach", "Cul-de-sac", 225_000, "Coelacanth", "Glass Beach", 394_000),
            None
        );
        assert_eq!(
            accept_track_candidate("The Doors", "Wild Child", 156_000, "Wild Child (Live at Aquarius)", "The Doors", 156_000),
            None
        );
        assert_eq!(
            accept_track_candidate("The Doors", "Wild Child", 156_000, "Wild Child", "The Doors", 320_000),
            None
        );
        assert_eq!(
            accept_track_candidate("The Doors", "Wild Child", 156_000, "Wild Child", "The Beatles", 156_000),
            None
        );
        assert!(
            accept_track_candidate("The Doors", "Wild Child", 156_000, "The Doors - Wild Child", "ClassicRockChannel", 156_000).is_some()
        );
        assert!(
            accept_track_candidate(
                "Glass Beach",
                "Cul-de-sac",
                225_000,
                "Cul-de-sac",
                "Glass Beach - Topic",
                225_000,
            )
            .is_some()
        );
        assert!(
            accept_track_candidate(
                "Glass Beach",
                "Cul-de-sac",
                225_000,
                "glass beach - cul-de-sac (Official Audio)",
                "IndieLabelUploader",
                226_000,
            )
            .is_some()
        );
    }
}
