use crate::search::ranking::normalize;
use reqwest::Client;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::task::JoinSet;

#[path = "providers/mod.rs"]
mod providers;

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;

const MAX_QUERY_FIELD_BYTES: usize = 512;
const MAX_RESULT_TITLE_BYTES: usize = 1536;

#[derive(Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub input: String,
    pub provider: String,
    pub providers: Vec<String>,
    pub info_hash: Option<String>,
    pub seeders: Option<u64>,
    pub size_bytes: Option<u64>,
    pub popularity: Option<u64>,
    pub rank: u8,
}

#[derive(Clone, Serialize)]
pub struct ProviderReport {
    pub provider: String,
    pub hits: Vec<SearchHit>,
    pub error: Option<String>,
    pub error_kind: Option<ProviderError>,
}

impl ProviderReport {
    fn from_hits(provider: String, hits: Vec<SearchHit>) -> Self {
        Self {
            provider,
            hits,
            error: None,
            error_kind: None,
        }
    }

    fn with_error(mut self, error: ProviderError) -> Self {
        self.error = Some(error.label().to_string());
        self.error_kind = Some(error);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderError {
    Malformed,
    Timeout,
    RateLimited,
    Unavailable,
    ResponseTooLarge,
}

impl ProviderError {
    fn label(self) -> &'static str {
        match self {
            Self::Malformed => "Invalid response from index",
            Self::Timeout => "Index request timed out",
            Self::RateLimited => "Index is rate limiting requests",
            Self::Unavailable => "Index unavailable",
            Self::ResponseTooLarge => "Index response exceeded the size limit",
        }
    }
}

#[derive(Serialize)]
pub struct SearchReport {
    pub providers: Vec<ProviderReport>,
    pub hits: Vec<SearchHit>,
}

#[derive(Clone)]
struct Query {
    artist: String,
    title: String,
    artist_words: Vec<String>,
    title_words: Vec<String>,
}

impl Query {
    fn new(artist: &str, title: &str) -> Result<Self, String> {
        let artist = clean_field(artist, MAX_QUERY_FIELD_BYTES)
            .ok_or_else(|| "Enter an artist for torrent discovery.".to_string())?;
        let title = clean_field(title, MAX_QUERY_FIELD_BYTES)
            .ok_or_else(|| "Enter a release or track title for torrent discovery.".to_string())?;
        let artist_words = normalize(&artist)
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        let title_words = normalize(&title)
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        if artist_words.is_empty() || title_words.is_empty() {
            return Err(
                "Torrent discovery needs an artist and title with letters or numbers.".into(),
            );
        }
        Ok(Self {
            artist,
            title,
            artist_words,
            title_words,
        })
    }

    fn text(&self) -> String {
        format!("{} {}", self.artist, self.title)
    }

    fn artist_prefix<'a>(&self, words: &'a [&str]) -> Option<&'a [&'a str]> {
        let (prefix, rest) = words.split_at_checked(self.artist_words.len())?;
        prefix
            .iter()
            .copied()
            .eq(self.artist_words.iter().map(String::as_str))
            .then_some(rest)
    }

    fn matches_creator(&self, creator: &str) -> bool {
        normalize(creator)
            .split_whitespace()
            .eq(self.artist_words.iter().map(String::as_str))
    }

    fn identity(&self, raw_title: &str, creators: &[&str]) -> Option<u8> {
        let normalized = normalize(raw_title);
        let words = normalized.split_whitespace().collect::<Vec<_>>();
        let leading_format = words.iter().take_while(|word| is_format_word(word)).count();
        self.artist_prefix(&words)
            .and_then(|tail| self.title_rank(tail))
            .or_else(|| {
                self.artist_prefix(&words[leading_format..])
                    .and_then(|tail| self.title_rank(tail))
            })
            .or_else(|| {
                creators
                    .iter()
                    .any(|creator| self.matches_creator(creator))
                    .then(|| self.title_rank(&words))
                    .flatten()
            })
    }

    fn title_rank(&self, words: &[&str]) -> Option<u8> {
        let title = self
            .title_words
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let after_year = words
            .split_first()
            .and_then(|(first, rest)| is_year(first).then_some(rest));
        std::iter::once(words)
            .chain(after_year)
            .filter_map(|candidate| candidate.strip_prefix(title.as_slice()))
            .find_map(suffix_rank)
    }
}

fn clean_field(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes {
        return None;
    }
    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!cleaned.chars().any(char::is_control)).then_some(cleaned)
}

fn is_year(word: &str) -> bool {
    word.len() == 4
        && word
            .parse::<u16>()
            .is_ok_and(|year| (1900..=2099).contains(&year))
}

fn is_format_word(word: &str) -> bool {
    matches!(
        word,
        "flac"
            | "mp3"
            | "aac"
            | "m4a"
            | "alac"
            | "ape"
            | "wav"
            | "wv"
            | "opus"
            | "ogg"
            | "lossless"
            | "web"
            | "webdl"
            | "vinyl"
            | "rip"
            | "eac"
            | "log"
            | "cue"
            | "v0"
            | "v2"
            | "cbr"
            | "vbr"
            | "kbps"
            | "khz"
            | "bit"
            | "16bit"
            | "24bit"
            | "32bit"
            | "320kbps"
            | "44khz"
            | "48khz"
            | "96khz"
            | "192khz"
            | "hires"
            | "stereo"
            | "mono"
    )
}

fn suffix_rank(suffix: &[&str]) -> Option<u8> {
    let mut rank = 3;
    for word in suffix {
        match *word {
            "deluxe" | "expanded" | "remaster" | "remastered" | "edition" | "reissue"
            | "anniversary" | "bonus" | "tracks" | "disc" | "disk" | "cd" | "cd1" | "cd2"
            | "cd3" => rank = 2,
            word if is_format_word(word) || is_year(word) => {}
            word if word.bytes().all(|byte| byte.is_ascii_digit())
                && suffix.iter().any(|word| {
                    is_format_word(word) || matches!(*word, "cd" | "disc" | "disk")
                }) => {}
            _ => return None,
        }
    }
    Some(rank)
}

fn sort_hits(hits: &mut [SearchHit]) {
    hits.sort_by(|left, right| {
        right
            .rank
            .cmp(&left.rank)
            .then_with(|| right.seeders.cmp(&left.seeders))
            .then_with(|| right.popularity.cmp(&left.popularity))
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.input.cmp(&right.input))
            .then_with(|| left.size_bytes.cmp(&right.size_bytes))
    });
}

fn merge_hits(mut hits: Vec<SearchHit>) -> Vec<SearchHit> {
    sort_hits(&mut hits);
    let mut unique = BTreeMap::<String, SearchHit>::new();
    for hit in hits {
        if let Some(existing) = unique.get_mut(&hit.id) {
            existing.providers.extend(hit.providers);
            existing.providers.sort_unstable();
            existing.providers.dedup();
            existing.seeders = existing.seeders.max(hit.seeders);
            existing.size_bytes = existing.size_bytes.or(hit.size_bytes);
            if !existing.input.contains("&xs=") && hit.input.contains("&xs=") {
                existing.input = hit.input;
            }
        } else {
            unique.insert(hit.id.clone(), hit);
        }
    }
    let mut hits = unique.into_values().collect::<Vec<_>>();
    sort_hits(&mut hits);
    hits
}

async fn collect(
    mut jobs: JoinSet<ProviderReport>,
    progress: impl Fn(ProviderReport),
) -> SearchReport {
    let mut providers = Vec::new();
    while let Some(result) = jobs.join_next().await {
        if let Ok(report) = result {
            progress(report.clone());
            providers.push(report);
        }
    }
    providers.sort_by(|left, right| left.provider.cmp(&right.provider));
    let hits = merge_hits(
        providers
            .iter()
            .flat_map(|provider| provider.hits.iter().cloned())
            .collect(),
    );
    SearchReport { providers, hits }
}

pub async fn search(
    artist: &str,
    title: &str,
    progress: impl Fn(ProviderReport),
) -> Result<SearchReport, String> {
    static CLIENT: OnceLock<Result<Client, reqwest::Error>> = OnceLock::new();
    let query = Query::new(artist, title)?;
    let client = CLIENT
        .get_or_init(|| {
            Client::builder()
                .user_agent(concat!("Blacktape/", env!("CARGO_PKG_VERSION")))
                .redirect(reqwest::redirect::Policy::none())
                .https_only(true)
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(7))
                .build()
        })
        .as_ref()
        .map_err(|_| "Could not initialize torrent search.".to_string())?;

    let mut jobs = JoinSet::new();
    for provider in providers::Provider::ALL {
        let query = query.clone();
        jobs.spawn(async move { provider.search(client, &query).await });
    }
    Ok(collect(jobs, progress).await)
}
