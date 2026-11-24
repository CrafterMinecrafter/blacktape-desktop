use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};
use crate::resolver::matcher::evaluate_track_match;
use crate::search::bandcamp::ItemType;
use tracing::{debug, warn};

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
        let eval = evaluate_track_match(title, duration_ms, &item.name, cand_duration_ms);

        // Keep candidates with decent confidence (> 0.40)
        if eval.confidence >= 0.40 {
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
                match_confidence: eval.confidence,
                is_strict_match: eval.is_strict_match,
                display_label: "YouTube (Opus 160k)".into(),
            });
        }
    }

    debug!(
        "Resolved {} YouTube candidate(s) for '{} - {}'",
        candidates.len(),
        artist,
        title
    );
    candidates
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
        let eval = evaluate_track_match(title, duration_ms, &item.name, cand_duration_ms);

        if eval.confidence >= 0.40 {
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
                match_confidence: eval.confidence,
                is_strict_match: eval.is_strict_match,
                display_label: "Bandcamp (MP3 128k)".into(),
            });
        }
    }

    debug!(
        "Resolved {} Bandcamp candidate(s) for '{} - {}'",
        candidates.len(),
        artist,
        title
    );
    candidates
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
        let eval = evaluate_track_match(title, duration_ms, &item.name, cand_duration_ms);

        if eval.confidence >= 0.40 {
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
                match_confidence: eval.confidence,
                is_strict_match: eval.is_strict_match,
                display_label: "SoundCloud (MP3 128k)".into(),
            });
        }
    }

    debug!(
        "Resolved {} SoundCloud candidate(s) for '{} - {}'",
        candidates.len(),
        artist,
        title
    );
    candidates
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
