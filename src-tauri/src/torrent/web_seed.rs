use librqbit::{torrent_from_bytes, ByteBuf, FileIteratorName};
use reqwest::{header, StatusCode};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::path::Path;
use tokio::io::AsyncWriteExt;
use url::Url;

const MAX_BLOCK: u64 = 8 * 1024 * 1024;
const MAX_REDIRECTS: usize = 4;

#[derive(Deserialize)]
#[serde(untagged)]
enum WebSeeds<'a> {
    One(#[serde(borrow)] ByteBuf<'a>),
    Many(#[serde(borrow)] Vec<ByteBuf<'a>>),
}

#[derive(Deserialize)]
struct SeedExtension<'a> {
    #[serde(rename = "url-list", borrow)]
    urls: Option<WebSeeds<'a>>,
}

struct SeedFile {
    start: u64,
    length: u64,
    url: Url,
    padding: bool,
}

pub(super) struct ArchiveSeed {
    files: Vec<SeedFile>,
    piece_length: u64,
    pieces: Vec<u8>,
    total: u64,
}

pub(super) fn archive_item(locator: &str) -> Option<String> {
    let url = Url::parse(locator).ok()?;
    if !allowed_url(&url) || url.host_str()? != "archive.org" {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    let ["download", item, file] = segments.as_slice() else { return None };
    let valid = !item.is_empty() && item.len() <= 128
        && item.as_bytes()[0].is_ascii_alphanumeric()
        && item.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && *file == format!("{item}_archive.torrent");
    valid.then(|| (*item).to_owned())
}

fn allowed_url(url: &Url) -> bool {
    url.scheme() == "https" && url.port().is_none()
        && url.username().is_empty() && url.password().is_none()
        && url.fragment().is_none() && url.query().is_none()
        && url.host_str().is_some_and(|host| host == "archive.org" || host.ends_with(".archive.org"))
}

fn seed_base(extension: SeedExtension<'_>) -> Option<Url> {
    let urls = match extension.urls? {
        WebSeeds::One(url) => vec![url],
        WebSeeds::Many(urls) => urls,
    };
    // Archive publishes both HTTP and HTTPS variants. Construct only the canonical HTTPS URL.
    urls.iter().any(|url| matches!(url.as_ref(), b"http://archive.org/download/" | b"https://archive.org/download/"))
        .then(|| Url::parse("https://archive.org/download/").ok()).flatten()
}

impl ArchiveSeed {
    pub(super) fn parse(bytes: &[u8], item: &str, hash: &str) -> Result<Self, String> {
        super::inspect::parse_metainfo(bytes, Some(hash))?;
        let extension = librqbit_bencode::from_bytes::<SeedExtension>(bytes)
            .map_err(|_| "Could not read torrent web seed metadata.")?;
        let base = seed_base(extension).ok_or("Torrent has no supported Archive web seed.")?;
        let info = torrent_from_bytes::<ByteBuf>(bytes)
            .map_err(|_| "Could not read torrent layout.")?.info;
        if info.name.as_ref().map(|name| name.as_ref()) != Some(item.as_bytes()) {
            return Err("Archive item does not match the torrent root.".into());
        }
        let piece_length = u64::from(info.piece_length);
        if piece_length == 0 || piece_length > MAX_BLOCK {
            return Err("Torrent piece length exceeds the web seed limit.".into());
        }
        let mut files = Vec::new();
        let mut total = 0u64;
        for file in info.iter_file_details().map_err(|_| "Invalid torrent file list.")? {
            let FileIteratorName::Tree(parts) = file.filename else {
                return Err("Archive web seed requires an item directory.".into());
            };
            let mut url = base.clone();
            {
                let mut segments = url.path_segments_mut().map_err(|_| "Invalid web seed base.")?;
                segments.pop_if_empty().push(item);
                for part in parts {
                    let part = std::str::from_utf8(part.as_ref()).map_err(|_| "Invalid web seed filename.")?;
                    if matches!(part, "." | "..") || part.contains(['/', '\\']) || part.is_empty() {
                        return Err("Unsafe web seed filename.".into());
                    }
                    segments.push(part);
                }
            }
            files.push(SeedFile { start: total, length: file.len, url, padding: file.attrs().padding });
            total = total.checked_add(file.len).ok_or("Torrent size overflow.")?;
        }
        let pieces = info.pieces.as_ref().to_vec();
        if pieces.len() % 20 != 0 || total.div_ceil(piece_length) != (pieces.len() / 20) as u64 {
            return Err("Torrent piece table does not match its file layout.".into());
        }
        Ok(Self { files, piece_length, pieces, total })
    }

    pub(super) async fn transfer<F, Fut>(
        &self, index: usize, size: u64, path: &Path,
        progress: F,
    ) -> Result<(), String>
    where
        F: FnMut(i32) -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let clients = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        self.transfer_with(index, size, path, progress, |url, range, size| {
            let clients = clients.clone();
            async move {
                fetch_range(&mut *clients.lock().await, &url, range, size).await
            }
        }).await
    }

    async fn transfer_with<F, Fut, Fetch, FetchFuture>(
        &self, index: usize, size: u64, path: &Path,
        mut progress: F, mut fetch: Fetch,
    ) -> Result<(), String>
    where
        F: FnMut(i32) -> Fut,
        Fut: std::future::Future<Output = bool>,
        Fetch: FnMut(Url, std::ops::Range<u64>, u64) -> FetchFuture,
        FetchFuture: std::future::Future<Output = Result<Vec<u8>, String>>,
    {
        let file = self.files.get(index).filter(|f| !f.padding && f.length == size && size > 0)
            .ok_or("Selected file does not match the web seed metadata.")?;
        let mut output = tokio::fs::File::create_new(path).await.map_err(|_| "Could not stage web seed data.")?;
        let end = file.start + file.length;
        let mut start = file.start / self.piece_length * self.piece_length;
        let limit = end.div_ceil(self.piece_length).saturating_mul(self.piece_length).min(self.total);
        let block_size = MAX_BLOCK / self.piece_length * self.piece_length;
        while start < limit {
            let percent = ((start.saturating_sub(file.start) as u128 * 100) / size as u128) as i32;
            if !progress(percent).await {
                return Err("Task cancelled".into());
            }
            let stop = start.saturating_add(block_size).min(limit);
            let mut block = Vec::with_capacity((stop - start) as usize);
            let first = self.files.partition_point(|entry| entry.start + entry.length <= start);
            for entry in self.files[first..].iter().take_while(|entry| entry.start < stop) {
                let from = start.max(entry.start) - entry.start;
                let to = stop.min(entry.start + entry.length) - entry.start;
                if from == to { continue; }
                if entry.padding {
                    block.resize(block.len() + (to - from) as usize, 0);
                } else {
                    let bytes = fetch(entry.url.clone(), from..to, entry.length).await?;
                    if bytes.len() as u64 != to - from {
                        return Err("Web seed returned an incomplete file segment.".into());
                    }
                    block.extend(bytes);
                }
            }
            self.verify(start, stop, &block)?;
            let selected = (file.start.max(start) - start) as usize..(end.min(stop) - start) as usize;
            output.write_all(&block[selected]).await.map_err(|_| "Could not stage web seed data.")?;
            start = stop;
        }
        output.flush().await.map_err(|_| "Could not flush web seed data.".into())
    }

    fn verify(&self, start: u64, stop: u64, block: &[u8]) -> Result<(), String> {
        if block.len() as u64 != stop - start {
            return Err("Web seed block has the wrong length.".into());
        }
        for (index, bytes) in block.chunks(self.piece_length as usize).enumerate() {
            let offset = (start / self.piece_length) as usize * 20 + index * 20;
            if self.pieces.get(offset..offset + 20) != Some(&Sha1::digest(bytes)[..]) {
                return Err("Web seed data failed torrent piece verification.".into());
            }
        }
        Ok(())
    }
}

async fn fetch_range(
    clients: &mut std::collections::HashMap<String, reqwest::Client>,
    original: &Url, range: std::ops::Range<u64>, size: u64,
) -> Result<Vec<u8>, String> {
    let mut url = original.clone();
    let len = (range.end - range.start) as usize;
    for _ in 0..=MAX_REDIRECTS {
        if !allowed_url(&url) { return Err("Unsafe Archive web seed redirect.".into()); }
        let host = url.host_str().ok_or("Invalid web seed host.")?.to_owned();
        if !clients.contains_key(&host) {
            clients.insert(host.clone(), super::inspect::public_client(&url).await?);
        }
        let mut response = clients[&host].get(url.clone())
            .header(header::ACCEPT_ENCODING, "identity")
            .header(header::RANGE, format!("bytes={}-{}", range.start, range.end - 1))
            .send().await.map_err(|_| "Archive web seed request failed.")?;
        if response.status().is_redirection() {
            url = response.headers().get(header::LOCATION).and_then(|v| v.to_str().ok())
                .and_then(|v| url.join(v).ok()).ok_or("Invalid Archive web seed redirect.")?;
            continue;
        }
        let expected = format!("bytes {}-{}/{size}", range.start, range.end - 1);
        if response.status() != StatusCode::PARTIAL_CONTENT
            || response.headers().get(header::CONTENT_RANGE).and_then(|v| v.to_str().ok()) != Some(&expected) {
            return Err("Archive web seed returned an invalid byte range.".into());
        }
        let mut bytes = Vec::with_capacity(len);
        while let Some(chunk) = response.chunk().await.map_err(|_| "Could not read Archive web seed data.")? {
            if chunk.len() > len - bytes.len() { return Err("Archive web seed exceeded its byte range.".into()); }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() != len { return Err("Archive web seed returned a short byte range.".into()); }
        return Ok(bytes);
    }
    Err("Archive web seed redirected too many times.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "uses the live Internet Archive catalog and web seed"]
    async fn live_archive_drukqs_web_seed_stages_verified_audio() {
        let locator = "https://archive.org/download/drukqs-flac/drukqs-flac_archive.torrent";
        let release = super::super::inspect::inspect(locator).await.unwrap();
        let track = release.tracks.iter().find(|track| {
            track.file_path.ends_with("01. Aphex Twin - Jynweythek.flac")
        }).expect("first album track should exist in this edition");
        let bytes = super::super::inspect::cached_torrent_bytes(locator).unwrap();
        let seed = ArchiveSeed::parse(&bytes, "drukqs-flac", &release.info_hash).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("first.flac");
        seed.transfer(track.file_index, track.size_bytes, &path, |_| std::future::ready(true)).await.unwrap();
        let audio = tokio::fs::read(path).await.unwrap();
        assert_eq!(audio.len() as u64, track.size_bytes);
        assert!(audio.starts_with(b"fLaC"));
    }

    #[test]
    fn archive_source_is_bound_to_the_item_and_allowed_hosts() {
        assert_eq!(archive_item("https://archive.org/download/my-item/my-item_archive.torrent").as_deref(), Some("my-item"));
        for locator in [
            "https://archive.org.evil.example/download/my-item/my-item_archive.torrent",
            "https://archive.org/download/my-item/other_archive.torrent",
            "http://archive.org/download/my-item/my-item_archive.torrent",
            "https://archive.org/download/my-item/my-item_archive.torrent?key=test",
        ] {
            assert!(archive_item(locator).is_none(), "{locator}");
        }
        assert!(allowed_url(&Url::parse("https://ia800001.us.archive.org/12/items/test/test.flac").unwrap()));
        assert!(!allowed_url(&Url::parse("https://archive.org@127.0.0.1/track.flac").unwrap()));
    }

    #[tokio::test]
    async fn archive_web_seed_transfers_only_selected_bytes_after_verifying_cross_file_pieces() {
        let mut bytes = b"d4:infod5:filesld6:lengthi2e4:pathl6:a.flaceed6:lengthi3e4:pathl6:b.flaceee4:name4:item12:piece lengthi4e6:pieces40:".to_vec();
        bytes.extend_from_slice(&Sha1::digest(b"abcd"));
        bytes.extend_from_slice(&Sha1::digest(b"e"));
        bytes.extend_from_slice(b"e8:url-listl29:https://archive.org/download/ee");
        let hash = torrent_from_bytes::<ByteBuf>(&bytes).unwrap().info_hash.as_string();
        let layout = ArchiveSeed::parse(&bytes, "item", &hash).unwrap();
        assert_eq!(layout.files[1].start, 2);
        assert_eq!(layout.files[1].url.as_str(), "https://archive.org/download/item/b.flac");
        assert!(layout.verify(0, 5, b"abcde").is_ok());
        assert!(layout.verify(0, 5, b"abCde").is_err());
        assert!(layout.verify(0, 5, b"abcd").is_err());
        assert!(ArchiveSeed::parse(&bytes, "other", &hash).is_err());
        assert!(ArchiveSeed::parse(&bytes, "item", "0000000000000000000000000000000000000000").is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("selected.flac");
        let fetch = |url: Url, range: std::ops::Range<u64>, _| async move {
            let bytes: &[u8] = if url.path().ends_with("a.flac") { b"ab" } else { b"cde" };
            Ok(bytes[range.start as usize..range.end as usize].to_vec())
        };
        layout.transfer_with(1, 3, &path, |_| std::future::ready(true), fetch).await.unwrap();
        assert_eq!(tokio::fs::read(path).await.unwrap(), b"cde");
        let cancelled = dir.path().join("cancelled.flac");
        assert_eq!(layout.transfer_with(1, 3, &cancelled, |_| std::future::ready(false), fetch).await.unwrap_err(), "Task cancelled");
        assert!(tokio::fs::read(cancelled).await.unwrap().is_empty());
    }
}
