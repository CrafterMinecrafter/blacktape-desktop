use crate::metadata::types::{CanonicalDisc, CanonicalRelease, CanonicalReleaseSummary, CanonicalTrack};
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

const MUSICBRAINZ_USER_AGENT: &str = "Blacktape/0.8.0 ( mail.seafood@proton.me; https://github.com/seafoodd/blacktape-desktop )";
const RATE_LIMIT_INTERVAL_MS: u64 = 1000;

#[derive(Clone)]
pub struct MusicBrainzClient {
    client: Client,
    cache_dir: Option<PathBuf>,
    last_request_time: Arc<Mutex<Option<Instant>>>,
}

impl MusicBrainzClient {
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        let client = Client::builder()
            .user_agent(MUSICBRAINZ_USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self {
            client,
            cache_dir,
            last_request_time: Arc::new(Mutex::new(None)),
        }
    }

    /// Enforces MusicBrainz's strict 1 request per second rate limit policy.
    async fn enforce_rate_limit(&self) {
        let mut last_time = self.last_request_time.lock().await;
        if let Some(prev) = *last_time {
            let elapsed = prev.elapsed();
            if elapsed < Duration::from_millis(RATE_LIMIT_INTERVAL_MS) {
                let delay = Duration::from_millis(RATE_LIMIT_INTERVAL_MS) - elapsed;
                tokio::time::sleep(delay).await;
            }
        }
        *last_time = Some(Instant::now());
    }

    fn cache_path(&self, key: &str) -> Option<PathBuf> {
        let dir = self.cache_dir.as_ref()?.join("musicbrainz");
        let hash = xxhash_rust::xxh3::xxh3_64(key.as_bytes());
        Some(dir.join(format!("{hash:016x}.json")))
    }

    pub async fn read_cache(&self, key: &str) -> Option<String> {
        let path = self.cache_path(key)?;
        if !path.is_file() {
            return None;
        }
        // Cache valid for 7 days
        if let Ok(metadata) = std::fs::metadata(&path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = modified.elapsed() {
                    if elapsed > Duration::from_secs(7 * 24 * 3600) {
                        return None;
                    }
                }
            }
        }
        tokio::fs::read_to_string(path).await.ok()
    }

    pub async fn write_cache(&self, key: &str, content: &str) {
        if let Some(path) = self.cache_path(key) {
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let _ = tokio::fs::write(path, content).await;
        }
    }

    /// Searches MusicBrainz for releases matching `query`.
    #[tracing::instrument(skip(self), fields(query = %query))]
    pub async fn search_releases(&self, query: &str) -> Result<Vec<MbReleaseSummary>, String> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let cache_key = format!("search:{}", trimmed.to_lowercase());
        if let Some(cached) = self.read_cache(&cache_key).await {
            if let Ok(parsed) = serde_json::from_str::<MbSearchResponse>(&cached) {
                tracing::debug!("Cache hit for MusicBrainz search query");
                return Ok(parsed.releases);
            }
        }

        self.enforce_rate_limit().await;

        let clean = trimmed.replace('"', " ").trim().to_string();
        let lucene_query = format!(r#"release:"{clean}" OR artist:"{clean}" OR "{clean}""#);
        let encoded = urlencoding_simple(&lucene_query);
        let url = format!("https://musicbrainz.org/ws/2/release/?query={encoded}&fmt=json&limit=10");

        tracing::info!(url = %url, "Fetching releases from MusicBrainz");
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("MusicBrainz search HTTP request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("MusicBrainz search returned status {}", resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read MusicBrainz response body: {e}"))?;

        self.write_cache(&cache_key, &body).await;

        let parsed: MbSearchResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse MusicBrainz JSON: {e}"))?;

        Ok(parsed.releases)
    }

    /// Fetches the full canonical release with complete tracklist, disc structures, and cover art.
    #[tracing::instrument(skip(self), fields(mbid = %mbid))]
    pub async fn fetch_canonical_release(&self, mbid: &str) -> Result<CanonicalRelease, String> {
        let cache_key = format!("release:{}", mbid);
        if let Some(cached) = self.read_cache(&cache_key).await {
            if let Ok(parsed) = serde_json::from_str::<CanonicalRelease>(&cached) {
                tracing::debug!("Cache hit for MusicBrainz canonical release");
                return Ok(parsed);
            }
        }

        if mbid.starts_with("synth:") || mbid.starts_with("http://") || mbid.starts_with("https://") {
            return Err(format!("Direct/synthesized URL '{mbid}' not found in cache"));
        }

        self.enforce_rate_limit().await;

        let url = format!("https://musicbrainz.org/ws/2/release/{mbid}?inc=recordings+artist-credits+media&fmt=json");
        tracing::info!(url = %url, "Fetching release details from MusicBrainz");

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("MusicBrainz release HTTP request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("MusicBrainz release returned status {}", resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read MusicBrainz release body: {e}"))?;

        let raw: MbReleaseDetails = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse MusicBrainz release details: {e}"))?;

        let cover_url = format!("https://coverartarchive.org/release/{mbid}/front-500");

        let release = parse_mb_release_details(raw, Some(cover_url));
        if let Ok(serialized) = serde_json::to_string(&release) {
            self.write_cache(&cache_key, &serialized).await;
        }

        Ok(release)
    }
}

fn urlencoding_simple(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

// -------------------------------------------------------------------------------------------------
// MusicBrainz JSON DTOs
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MbSearchResponse {
    #[serde(default)]
    pub releases: Vec<MbReleaseSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MbReleaseSummary {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default, rename = "artist-credit")]
    pub artist_credit: Vec<MbArtistCredit>,
    #[serde(default, rename = "track-count")]
    pub track_count: Option<usize>,
}

impl MbReleaseSummary {
    pub fn into_canonical_summary(self) -> CanonicalReleaseSummary {
        let (artist, _) = format_artist_credit(&self.artist_credit);
        let year = self
            .date
            .as_deref()
            .and_then(|d| d.split('-').next())
            .and_then(|y| y.parse::<u32>().ok());
        let cover_url = format!("https://coverartarchive.org/release/{}/front-250", self.id);
        CanonicalReleaseSummary {
            id: self.id.clone(),
            mbid: Some(self.id),
            title: self.title,
            artist,
            year,
            track_count: self.track_count.unwrap_or(0),
            cover_url: Some(cover_url),
            metadata_source: "musicbrainz".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MbArtistCredit {
    pub name: String,
    pub joinphrase: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MbReleaseDetails {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default, rename = "artist-credit")]
    pub artist_credit: Vec<MbArtistCredit>,
    #[serde(default)]
    pub media: Vec<MbMedium>,
}

#[derive(Debug, Deserialize)]
pub struct MbMedium {
    pub position: u32,
    pub title: Option<String>,
    #[serde(default)]
    pub tracks: Vec<MbTrack>,
}

#[derive(Debug, Deserialize)]
pub struct MbTrack {
    pub position: u32,
    pub title: String,
    #[serde(default)]
    pub length: Option<u64>,
    #[serde(default, rename = "artist-credit")]
    pub artist_credit: Vec<MbArtistCredit>,
    #[serde(default)]
    pub recording: Option<MbRecording>,
}

#[derive(Debug, Deserialize)]
pub struct MbRecording {
    pub id: String,
    pub title: Option<String>,
    #[serde(default)]
    pub length: Option<u64>,
}

// -------------------------------------------------------------------------------------------------
// Parsing & Mapping Logic
// -------------------------------------------------------------------------------------------------

pub fn format_artist_credit(credits: &[MbArtistCredit]) -> (String, Vec<String>) {
    if credits.is_empty() {
        return ("Unknown Artist".to_string(), vec!["Unknown Artist".to_string()]);
    }

    let mut full = String::new();
    let mut names = Vec::new();

    for credit in credits {
        full.push_str(&credit.name);
        if let Some(ref join) = credit.joinphrase {
            full.push_str(join);
        }
        names.push(credit.name.clone());
    }

    (full, names)
}

pub fn parse_mb_release_details(raw: MbReleaseDetails, cover_url: Option<String>) -> CanonicalRelease {
    let (album_artist, _) = format_artist_credit(&raw.artist_credit);
    let is_va = album_artist.eq_ignore_ascii_case("Various Artists");

    let release_year = raw
        .date
        .as_deref()
        .and_then(|d| d.split('-').next())
        .and_then(|y| y.parse::<u32>().ok());

    let mut discs = Vec::new();
    let mut total_tracks = 0;

    for medium in raw.media {
        let mut tracks = Vec::new();

        for track in medium.tracks {
            let (_track_artist_full, track_artists) = format_artist_credit(&track.artist_credit);
            let artists = if track_artists.is_empty() || (track_artists.len() == 1 && track_artists[0] == "Unknown Artist") {
                vec![album_artist.clone()]
            } else {
                track_artists
            };

            let duration_ms = track
                .length
                .or_else(|| track.recording.as_ref().and_then(|r| r.length))
                .unwrap_or(0);

            tracks.push(CanonicalTrack {
                track_number: track.position,
                disc_number: medium.position,
                title: track.title,
                artists,
                duration_ms,
                isrc: None,
                candidates: Vec::new(),
                selected_candidate_idx: 0,
            });
            total_tracks += 1;
        }

        discs.push(CanonicalDisc {
            disc_number: medium.position,
            title: medium.title.filter(|t| !t.trim().is_empty()),
            tracks,
        });
    }

    let total_discs = discs.len();

    CanonicalRelease {
        id: raw.id.clone(),
        mbid: Some(raw.id),
        title: raw.title,
        album_artist,
        is_various_artists: is_va,
        release_year,
        total_tracks,
        total_discs,
        discs,
        cover_url,
        metadata_source: "musicbrainz".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_artist_multi_disc_release() {
        let json = r#"{
            "id": "mbid-1234",
            "title": "Selected Ambient Works Volume II",
            "date": "1994-03-07",
            "artist-credit": [{ "name": "Aphex Twin", "joinphrase": null }],
            "media": [
                {
                    "position": 1,
                    "title": "Disc 1",
                    "tracks": [
                        { "position": 1, "title": "Cliffs", "length": 444000, "artist-credit": [{ "name": "Aphex Twin", "joinphrase": null }] },
                        { "position": 2, "title": "Radiator", "length": 394000, "artist-credit": [{ "name": "Aphex Twin", "joinphrase": null }] }
                    ]
                },
                {
                    "position": 2,
                    "title": "Disc 2",
                    "tracks": [
                        { "position": 1, "title": "Blue Calx", "length": 440000, "artist-credit": [{ "name": "Aphex Twin", "joinphrase": null }] }
                    ]
                }
            ]
        }"#;

        let details: MbReleaseDetails = serde_json::from_str(json).unwrap();
        let release = parse_mb_release_details(details, Some("https://cover.test/front.jpg".into()));

        assert_eq!(release.id, "mbid-1234");
        assert_eq!(release.mbid.as_deref(), Some("mbid-1234"));
        assert_eq!(release.title, "Selected Ambient Works Volume II");
        assert_eq!(release.album_artist, "Aphex Twin");
        assert!(!release.is_various_artists);
        assert_eq!(release.release_year, Some(1994));
        assert_eq!(release.total_discs, 2);
        assert_eq!(release.total_tracks, 3);
        assert_eq!(release.discs[0].tracks.len(), 2);
        assert_eq!(release.discs[1].tracks.len(), 1);
        assert_eq!(release.discs[0].tracks[0].title, "Cliffs");
        assert_eq!(release.discs[0].tracks[0].duration_ms, 444000);
        assert_eq!(release.cover_url.as_deref(), Some("https://cover.test/front.jpg"));
    }

    #[test]
    fn parses_various_artists_compilation() {
        let json = r#"{
            "id": "mbid-va-8mile",
            "title": "8 Mile: Music from and Inspired by the Motion Picture",
            "date": "2002-10-29",
            "artist-credit": [{ "name": "Various Artists", "joinphrase": null }],
            "media": [
                {
                    "position": 1,
                    "title": null,
                    "tracks": [
                        { "position": 1, "title": "Lose Yourself", "length": 326000, "artist-credit": [{ "name": "Eminem", "joinphrase": null }] },
                        { "position": 2, "title": "Love Me", "length": 270000, "artist-credit": [{ "name": "Obie Trice", "joinphrase": ", " }, { "name": "50 Cent", "joinphrase": " & " }, { "name": "Eminem", "joinphrase": null }] }
                    ]
                }
            ]
        }"#;

        let details: MbReleaseDetails = serde_json::from_str(json).unwrap();
        let release = parse_mb_release_details(details, None);

        assert!(release.is_various_artists);
        assert_eq!(release.album_artist, "Various Artists");
        assert_eq!(release.discs[0].tracks[0].artists, vec!["Eminem"]);
        assert_eq!(release.discs[0].tracks[1].artists, vec!["Obie Trice", "50 Cent", "Eminem"]);
    }
}
