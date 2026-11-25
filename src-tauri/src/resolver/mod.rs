pub mod arbiter;
pub mod matcher;
pub mod provider;

use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource, CanonicalRelease, CanonicalTrack};
use tauri::Emitter;
use tracing::info;

/// Resolves audio candidates for an entire release concurrently:
/// First performs fast album-level resolution across platforms, then falls back to track-level probes.
/// Emits live "album-track-candidates" events to the frontend as tracks are resolved.
pub async fn resolve_release_candidates(
    release: &mut CanonicalRelease,
    app_handle: Option<&tauri::AppHandle>,
    album_url: &str,
) {
    let artist = release.album_artist.clone();
    let album_title = release.title.clone();

    // Step 1: Query full album tracks concurrently across YouTube, Bandcamp, and SoundCloud
    let yt_fut = provider::resolve_youtube_album_tracks(&artist, &album_title);
    let bc_fut = provider::resolve_bandcamp_album_tracks(&artist, &album_title);
    let sc_fut = provider::resolve_soundcloud_album_tracks(&artist, &album_title);

    let (yt_tracks, bc_tracks, sc_tracks) = tokio::join!(yt_fut, bc_fut, sc_fut);

    let mut global_track_idx = 0;
    for disc in &mut release.discs {
        for track in &mut disc.tracks {
            let mut candidates = track.candidates.clone();

            // Match against YouTube album
            if let Some(yt_t) = matcher::find_matching_track(&track.title, track.duration_ms, track.track_number, &yt_tracks) {
                let cand_duration = yt_t.duration_ms.unwrap_or(0);
                let eval = matcher::evaluate_track_match(&track.title, track.duration_ms, &yt_t.title, cand_duration);
                candidates.push(AudioCandidate {
                    source: AudioSource::HttpStream {
                        platform: "youtube".into(),
                        url: yt_t.url.clone(),
                        format_id: None,
                        direct_stream_url: None,
                        expires_at: None,
                    },
                    quality_tier: AudioQualityTier::HighLossy,
                    codec: "Opus".into(),
                    bitrate_kbps: Some(160),
                    sample_rate_hz: Some(48000),
                    duration_ms: cand_duration,
                    match_confidence: eval.confidence,
                    is_strict_match: eval.is_strict_match,
                    display_label: "YouTube Music (Opus 160k)".into(),
                });
            }

            // Match against Bandcamp album
            if let Some(bc_t) = matcher::find_matching_track(&track.title, track.duration_ms, track.track_number, &bc_tracks) {
                let cand_duration = bc_t.duration_ms.unwrap_or(0);
                let eval = matcher::evaluate_track_match(&track.title, track.duration_ms, &bc_t.title, cand_duration);
                candidates.push(AudioCandidate {
                    source: AudioSource::HttpStream {
                        platform: "bandcamp".into(),
                        url: bc_t.url.clone(),
                        format_id: None,
                        direct_stream_url: None,
                        expires_at: None,
                    },
                    quality_tier: AudioQualityTier::StandardLossy,
                    codec: "MP3".into(),
                    bitrate_kbps: Some(128),
                    sample_rate_hz: Some(44100),
                    duration_ms: cand_duration,
                    match_confidence: eval.confidence,
                    is_strict_match: eval.is_strict_match,
                    display_label: "Bandcamp (MP3 128k)".into(),
                });
            }

            // Match against SoundCloud album
            if let Some(sc_t) = matcher::find_matching_track(&track.title, track.duration_ms, track.track_number, &sc_tracks) {
                let cand_duration = sc_t.duration_ms.unwrap_or(0);
                let eval = matcher::evaluate_track_match(&track.title, track.duration_ms, &sc_t.title, cand_duration);
                candidates.push(AudioCandidate {
                    source: AudioSource::HttpStream {
                        platform: "soundcloud".into(),
                        url: sc_t.url.clone(),
                        format_id: None,
                        direct_stream_url: None,
                        expires_at: None,
                    },
                    quality_tier: AudioQualityTier::StandardLossy,
                    codec: "MP3".into(),
                    bitrate_kbps: Some(128),
                    sample_rate_hz: Some(44100),
                    duration_ms: cand_duration,
                    match_confidence: eval.confidence,
                    is_strict_match: eval.is_strict_match,
                    display_label: "SoundCloud (MP3 128k)".into(),
                });
            }

            // If candidates are still empty (e.g. tracks not matched in album), fall back to single-track probes
            if candidates.is_empty() {
                let t_artist = track.artists.first().map(|s| s.as_str()).unwrap_or(&artist);
                let probed = resolve_candidates_for_track(t_artist, &track.title, track.duration_ms).await;
                candidates.extend(probed);
            }

            // Deduplicate by source URL
            let mut unique = Vec::new();
            for c in candidates {
                if !unique.iter().any(|u: &AudioCandidate| u.source.primary_url() == c.source.primary_url()) {
                    unique.push(c);
                }
            }
            arbiter::rank_candidates(&mut unique);
            track.candidates = unique;
            track.selected_candidate_idx = 0;

            if track.duration_ms == 0 {
                if let Some(inferred_ms) = track
                    .candidates
                    .iter()
                    .find(|c| c.duration_ms > 0)
                    .map(|c| c.duration_ms)
                {
                    track.duration_ms = inferred_ms;
                }
            }

            if let Some(app) = app_handle {
                let _ = app.emit(
                    "album-track-candidates",
                    serde_json::json!({
                        "album_url": album_url,
                        "track_index": global_track_idx,
                        "candidates": track.candidates,
                        "selected_candidate_idx": 0,
                        "duration_ms": (track.duration_ms > 0).then_some(track.duration_ms),
                    }),
                );
            }
            global_track_idx += 1;
        }
    }
}

/// Resolves audio candidates for a track across all supported platforms concurrently.
/// Ranks results strictly by: Strict Match -> Quality Tier -> Bitrate -> Confidence.
pub async fn resolve_candidates_for_track(
    artist: &str,
    title: &str,
    duration_ms: u64,
) -> Vec<AudioCandidate> {
    info!(
        "Resolving multi-source candidates for '{} - {}' (duration: {}ms)",
        artist, title, duration_ms
    );

    let yt_fut = provider::resolve_youtube_candidates(artist, title, duration_ms);
    let bc_fut = provider::resolve_bandcamp_candidates(artist, title, duration_ms);
    let sc_fut = provider::resolve_soundcloud_candidates(artist, title, duration_ms);
    let tor_fut = provider::resolve_torrent_candidates(artist, title, duration_ms);

    let (yt_res, bc_res, sc_res, tor_res) = tokio::join!(yt_fut, bc_fut, sc_fut, tor_fut);

    let mut all_candidates = Vec::new();
    all_candidates.extend(yt_res);
    all_candidates.extend(bc_res);
    all_candidates.extend(sc_res);
    all_candidates.extend(tor_res);

    arbiter::rank_candidates(&mut all_candidates);
    all_candidates
}

/// Populates a canonical track with resolved candidates.
pub async fn populate_track_candidates(track: &mut CanonicalTrack, fallback_artist: Option<&str>) {
    let artist = track
        .artists
        .first()
        .map(|s| s.as_str())
        .or(fallback_artist)
        .unwrap_or("Unknown Artist");

    let candidates = resolve_candidates_for_track(artist, &track.title, track.duration_ms).await;
    track.candidates = candidates;
    track.selected_candidate_idx = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::types::{AudioQualityTier, AudioSource};

    #[tokio::test]
    async fn populates_track_and_defaults_to_first_candidate() {
        let mut track = CanonicalTrack {
            track_number: 1,
            disc_number: 1,
            title: "Test Track".into(),
            artists: vec!["Test Artist".into()],
            duration_ms: 180000,
            isrc: None,
            candidates: Vec::new(),
            selected_candidate_idx: 0,
        };

        // When populated manually or via resolver:
        track.candidates = vec![
            AudioCandidate {
                source: AudioSource::HttpStream {
                    platform: "youtube".into(),
                    url: "https://music.youtube.com/watch?v=1".into(),
                    format_id: None,
                    direct_stream_url: None,
                    expires_at: None,
                },
                quality_tier: AudioQualityTier::HighLossy,
                codec: "Opus".into(),
                bitrate_kbps: Some(160),
                sample_rate_hz: Some(48000),
                duration_ms: 180000,
                match_confidence: 0.98,
                is_strict_match: true,
                display_label: "YouTube (Opus 160k)".into(),
            },
        ];

        assert_eq!(track.selected_candidate_idx, 0);
        let selected = track.selected_candidate().unwrap();
        assert_eq!(selected.codec, "Opus");
        assert_eq!(selected.quality_tier, AudioQualityTier::HighLossy);
    }
}
