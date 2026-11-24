pub mod arbiter;
pub mod matcher;
pub mod provider;

use crate::metadata::types::{AudioCandidate, CanonicalTrack};
use tracing::info;

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
