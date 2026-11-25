use super::{bandcamp, soundcloud, youtube};
use crate::download::album::download_album_cover;
use crate::download::types::{AlbumMetaEvent, DownloadResult, ProgressEvent, TrackMetaItem};
use crate::download::ytdlp;
use crate::types::Platform;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

pub async fn process_track_url(
    platform: Platform,
    url: &str,
    output_dir: &str,
    app_handle: &AppHandle,
    task_id: &str,
) -> Result<DownloadResult, String> {
    let _ = app_handle.emit(
        "download-task-progress",
        ProgressEvent {
            task_id: task_id.to_string(),
            current: 0,
            total: 1,
            track_title: "Fetching track metadata...".into(),
        },
    );

    let registry = app_handle.try_state::<crate::download::TaskRegistry>();
    let effective_format = match &registry {
        Some(r) => r.format(task_id).await,
        None => None,
    };
    let replace_song_id = match &registry {
        Some(r) => r.replace_song_id(task_id).await,
        None => None,
    };

    let (mut track_info, external_cover_url) = if url.starts_with("resolve:") {
        let parsed_id = url
            .strip_prefix("resolve:song:")
            .and_then(|s| s.parse::<i64>().ok())
            .or(replace_song_id);

        let old_song = if let (Some(id), Some(db_state)) = (
            parsed_id,
            app_handle.try_state::<tokio::sync::Mutex<crate::db::db::Database>>(),
        ) {
            let db = db_state.lock().await;
            db.get_song_by_id(id).await.ok().flatten()
        } else {
            None
        };

        let existing_song = old_song.ok_or_else(|| {
            "Multi-source track upgrade requires an existing library song record.".to_string()
        })?;

        let search_artist = existing_song
            .artists
            .first()
            .filter(|a| !a.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| existing_song.album_artist.clone());

        let candidates = crate::resolver::resolve_candidates_for_track(
            &search_artist,
            &existing_song.title,
            existing_song.duration_ms,
        )
        .await;

        let local_exists = std::path::Path::new(&existing_song.path).exists();
        let chosen = crate::download::album::pick_candidate_entry_for_tier(
            &candidates,
            effective_format.as_deref(),
            0,
        );
        if let Some((chosen_idx, _, _)) = &chosen {
            let cand_tier = candidates[*chosen_idx].quality_tier;
            if !crate::download::album::candidate_improves_local(
                existing_song.quality_tier,
                local_exists,
                cand_tier,
            ) {
                tracing::info!(
                    title = %existing_song.title,
                    local_tier = ?existing_song.quality_tier,
                    candidate_tier = ?cand_tier,
                    "Skipping single-track upgrade: local file already has equal or higher quality"
                );
                let _ = app_handle.emit(
                    "download-task-progress",
                    ProgressEvent {
                        task_id: task_id.to_string(),
                        current: 1,
                        total: 1,
                        track_title: "Already at the best available quality".into(),
                    },
                );
                return Ok((PathBuf::from(output_dir), Vec::new(), None));
            }
        } else if local_exists {
            return Ok((PathBuf::from(output_dir), Vec::new(), None));
        }

        let (mut primary_url, mut fallback_urls) = match chosen {
            Some((_, primary, fallbacks)) => (primary, fallbacks),
            None => (String::new(), Vec::new()),
        };

        if let Some(ref orig_url) = existing_song.source_url {
            if orig_url.starts_with("http://") || orig_url.starts_with("https://") {
                if primary_url.is_empty() {
                    primary_url = orig_url.clone();
                } else if orig_url != &primary_url && !fallback_urls.contains(orig_url) {
                    fallback_urls.push(orig_url.clone());
                }
            }
        }

        if primary_url.is_empty() {
            return Err(format!(
                "Could not find a matching stream on YouTube, Bandcamp, or SoundCloud for '{}'.",
                existing_song.title
            ));
        }

        let td = crate::download::types::TrackDownload {
            url: primary_url,
            file_name: String::new(),
            title: existing_song.title.clone(),
            artists: existing_song.artists.clone(),
            album_artist: existing_song.album_artist.clone(),
            album: existing_song.album.clone(),
            track_number: existing_song.track_number,
            disc_number: existing_song.disc_number,
            genres: existing_song
                .genre
                .as_ref()
                .map(|g| vec![g.clone()]),
            release_year: existing_song.release_year,
            source_item_id: None,
            duration_ms: Some(existing_song.duration_ms),
            fallback_urls,
        };
        (td, existing_song.external_cover_url)
    } else {
        match platform {
            Platform::Bandcamp => bandcamp::parse_track(url).await?,
            Platform::Youtube => youtube::parse_track(url).await?,
            Platform::Soundcloud => soundcloud::parse_track(url).await?,
            Platform::Local => {
                return Err("Local files cannot be processed via remote URL pipelines.".into());
            }
        }
    };

    // The library already has the album of this track (e.g. downloaded from another platform): file it there.
    if track_info.track_number.is_none() {
        if let Some(db_state) = app_handle.try_state::<tokio::sync::Mutex<crate::db::db::Database>>() {
            let artist = track_info.artists.first().cloned().unwrap_or_else(|| track_info.album_artist.clone());
            let db = db_state.lock().await;
            if let Ok(rows) = db.list_album_sources_for_artist(&artist).await {
                if let Some((album, album_artist, number)) =
                    crate::library_index::sources::find_in_sources(&rows, &artist, &track_info.title)
                {
                    track_info.album = album;
                    track_info.album_artist = album_artist;
                    track_info.track_number = Some(number);
                }
            }
        }
    }

    let album_title = if track_info.album.is_empty() {
        "Singles".to_string()
    } else {
        track_info.album.clone()
    };

    if let Some(registry) = app_handle.try_state::<crate::download::TaskRegistry>() {
        registry
            .update_metadata(
                task_id,
                &album_title,
                &track_info.album_artist,
                external_cover_url.as_deref(),
                track_info.release_year,
                vec![(track_info.title.clone(), track_info.track_number, track_info.disc_number)],
            )
            .await;
    }

    let _ = app_handle.emit(
        "download-album-meta",
        AlbumMetaEvent {
            task_id: task_id.to_string(),
            title: album_title,
            artist: track_info.album_artist.clone(),
            cover_url: external_cover_url.clone(),
            release_year: track_info.release_year,
            tracks: vec![TrackMetaItem {
                title: track_info.title.clone(),
                track_number: track_info.track_number.or(Some(1)),
                disc_number: track_info.disc_number,
            }],
        },
    );

    let (temp_file_path, downloaded_track) =
        ytdlp::download_single(app_handle, &track_info, output_dir, task_id).await?;

    let mut cover_path = None;
    if let Some(cover_url) = external_cover_url {
        let temp_dir = temp_file_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(output_dir));

        if download_album_cover(&cover_url, &temp_dir).await.is_ok() {
            cover_path = Some(temp_dir.join("cover.jpg"));
        }
    }

    let root_library_dir = PathBuf::from(output_dir);
    Ok((
        root_library_dir,
        vec![(temp_file_path, downloaded_track)],
        cover_path,
    ))
}
