use crate::download::types::TrackDownload;
use librqbit::{AddTorrent, AddTorrentOptions, ManagedTorrent, Session, SessionOptions};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use uuid::Uuid;

const TORRENT_INIT_TIMEOUT: Duration = Duration::from_secs(25);
const TORRENT_FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(25);
const TORRENT_STALL_TIMEOUT: Duration = Duration::from_secs(45);
const POLL_INTERVAL: Duration = Duration::from_millis(350);

#[derive(Debug, Clone)]
pub struct TorrentFileRequest {
    pub track_index: usize,
    pub file_index: usize,
    pub file_size: u64,
    pub track: TrackDownload,
}

#[derive(Debug, Clone)]
pub struct TorrentTransferResult {
    pub completed: Vec<(PathBuf, TrackDownload, usize)>,
    pub failed_tracks: Vec<(usize, TrackDownload)>,
}

#[derive(Clone, PartialEq, Eq)]
struct SessionConfig {
    cache_root: PathBuf,
    disable_dht: bool,
}

static SESSION: OnceLock<Mutex<Option<(SessionConfig, Arc<Session>)>>> = OnceLock::new();

async fn reset_session() {
    let lock = SESSION.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().await;
    *guard = None;
}

async fn get_or_init_session(cache_root: PathBuf, disable_dht: bool) -> Result<Arc<Session>, String> {
    let desired = SessionConfig {
        cache_root: cache_root.clone(),
        disable_dht,
    };
    let lock = SESSION.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().await;
    if let Some((ref existing_cfg, ref session)) = *guard {
        if *existing_cfg == desired {
            return Ok(Arc::clone(session));
        }
    }

    tokio::fs::create_dir_all(&cache_root)
        .await
        .map_err(|e| format!("Failed creating torrent cache dir: {e}"))?;

    let mut opts = SessionOptions::default();
    opts.disable_dht = disable_dht;
    opts.disable_dht_persistence = true;
    opts.fastresume = false;
    opts.persistence = None;
    opts.disable_upload = true;

    let session = match Session::new_with_opts(cache_root.clone(), opts).await {
        Ok(s) => s,
        Err(err) if !disable_dht => {
            tracing::warn!(error = %err, "Failed starting torrent session with DHT; retrying without DHT");
            let mut fallback_opts = SessionOptions::default();
            fallback_opts.disable_dht = true;
            fallback_opts.disable_dht_persistence = true;
            fallback_opts.fastresume = false;
            fallback_opts.persistence = None;
            fallback_opts.disable_upload = true;
            Session::new_with_opts(cache_root.clone(), fallback_opts)
                .await
                .map_err(|e| format!("Failed starting BitTorrent session: {e}"))?
        }
        Err(err) => return Err(format!("Failed starting BitTorrent session: {err}")),
    };
    *guard = Some((desired, Arc::clone(&session)));
    Ok(session)
}

fn validate_relative_path(rel: &std::path::Path) -> Result<(), String> {
    use std::path::Component;
    if rel.is_absolute() || rel.as_os_str().is_empty() {
        return Err("Unsafe absolute torrent file path.".into());
    }
    for comp in rel.components() {
        match comp {
            Component::Normal(_) => {}
            _ => return Err("Unsafe traversal component in torrent file path.".into()),
        }
    }
    Ok(())
}

/// Downloads selected files from one torrent (`magnet_uri` or validated `.torrent` URL) using
/// `librqbit::Session`, emitting `download-track-progress` events and forwarding each completed
/// track to `track_sender` immediately so tagging & DB import happen pipelined.
pub async fn download_torrent_files(
    app: &AppHandle,
    task_id: &str,
    library_dir: &std::path::Path,
    locator: &str,
    requests: &[TorrentFileRequest],
    total_task_tracks: usize,
    completed_counter: Arc<std::sync::atomic::AtomicUsize>,
    track_sender: Option<tokio::sync::mpsc::Sender<(PathBuf, TrackDownload, usize)>>,
) -> Result<TorrentTransferResult, String> {
    use std::sync::atomic::Ordering;

    if requests.is_empty() {
        return Ok(TorrentTransferResult {
            completed: Vec::new(),
            failed_tracks: Vec::new(),
        });
    }

    // Validate scheme before handing locator to librqbit.
    super::inspect::normalize_locator_for_download(locator)?;

    let temp_root = library_dir.join(".temp");
    let _ = tokio::fs::create_dir_all(&temp_root).await;
    let _ = crate::utils::set_hidden(&temp_root, true);
    let cache_root = torrent_cache_root(library_dir);
    let job_dir = cache_root.join(format!("job-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&job_dir)
        .await
        .map_err(|e| format!("Failed creating torrent job folder: {e}"))?;
    let staging_dir = torrent_staging_dir(&cache_root, task_id);
    let _ = tokio::fs::create_dir_all(&staging_dir).await;

    let mut only_files: Vec<usize> = requests.iter().map(|r| r.file_index).collect();
    only_files.sort_unstable();
    only_files.dedup();

    let (session, handle) = add_torrent_with_recovery(
        app,
        task_id,
        &cache_root,
        &job_dir,
        locator,
        &only_files,
        total_task_tracks,
    )
    .await?;
    let torrent_id = handle.id();

    let init_started = Instant::now();
    let mut last_status_at = Instant::now();
    let init_res: Result<(), String> = loop {
        if is_task_cancelled(app, task_id).await {
            break Err("Task cancelled".to_string());
        }
        if let Ok(Ok(())) = tokio::time::timeout(
            Duration::from_millis(250),
            handle.wait_until_initialized(),
        )
        .await
        {
            break Ok(());
        }
        let elapsed = init_started.elapsed();
        if elapsed >= TORRENT_INIT_TIMEOUT {
            break Err("Timed out waiting for torrent peers/metadata.".to_string());
        }
        if last_status_at.elapsed() >= Duration::from_secs(2) {
            last_status_at = Instant::now();
            emit_task_progress(
                app,
                task_id,
                completed_counter.load(Ordering::SeqCst),
                total_task_tracks,
                &format!("Waiting for torrent metadata ({}s)…", elapsed.as_secs()),
            )
            .await;
        }
    };

    if let Err(msg) = init_res {
        let _ = session.delete(torrent_id.into(), true).await;
        let _ = tokio::fs::remove_dir_all(&job_dir).await;
        return Err(msg);
    }

    emit_task_progress(
        app,
        task_id,
        completed_counter.load(Ordering::SeqCst),
        total_task_tracks,
        "Waiting for torrent seeders…",
    )
    .await;

    let file_rel_paths: HashMap<usize, (PathBuf, u64)> = match handle.with_metadata(|meta| {
        requests
            .iter()
            .filter_map(|req| {
                let fi = meta.file_infos.get(req.file_index)?;
                Some((req.file_index, (fi.relative_filename.clone(), fi.len)))
            })
            .collect()
    }) {
        Ok(map) => map,
        Err(e) => {
            let _ = session.delete(torrent_id.into(), true).await;
            let _ = tokio::fs::remove_dir_all(&job_dir).await;
            return Err(format!("Torrent metadata unavailable: {e}"));
        }
    };

    for (rel, _) in file_rel_paths.values() {
        if let Err(err) = validate_relative_path(rel) {
            let _ = session.delete(torrent_id.into(), true).await;
            let _ = tokio::fs::remove_dir_all(&job_dir).await;
            return Err(err);
        }
    }

    let mut done_indices = HashSet::<usize>::new();
    let mut last_pct: HashMap<usize, i32> = HashMap::new();
    let mut completed = Vec::new();
    let mut failed_tracks = Vec::new();
    let mut last_progress_bytes = 0u64;
    let mut last_progress_at = Instant::now();
    let mut last_wait_notice_at = Instant::now();
    let mut cancelled = false;
    let mut transfer_failure: Option<String> = None;

    loop {
        if is_task_cancelled(app, task_id).await {
            cancelled = true;
            break;
        }

        let stats = handle.stats();
        if let Some(err) = &stats.error {
            tracing::warn!(task_id, error = %err, "Torrent transfer reported error");
            transfer_failure = Some(format!("Torrent transfer error: {err}"));
            break;
        }

        if stats.progress_bytes > last_progress_bytes {
            last_progress_bytes = stats.progress_bytes;
            last_progress_at = Instant::now();
        } else {
            let stall_limit = if last_progress_bytes == 0 {
                TORRENT_FIRST_BYTE_TIMEOUT
            } else {
                TORRENT_STALL_TIMEOUT
            };
            if last_progress_at.elapsed() >= stall_limit {
                tracing::warn!(task_id, bytes = last_progress_bytes, "Torrent download stalled without peer progress");
                transfer_failure = Some(if last_progress_bytes == 0 {
                    "Timed out connecting to torrent peers (no active seeders responded).".into()
                } else {
                    "Torrent download stalled while waiting for remaining data.".into()
                });
                break;
            }
            if last_progress_bytes == 0 && last_wait_notice_at.elapsed() >= Duration::from_secs(3) {
                last_wait_notice_at = Instant::now();
                emit_task_progress(
                    app,
                    task_id,
                    completed_counter.load(Ordering::SeqCst),
                    total_task_tracks,
                    &format!(
                        "Waiting for torrent seeders ({}s)…",
                        last_progress_at.elapsed().as_secs()
                    ),
                )
                .await;
            }
        }

        for req in requests {
            if done_indices.contains(&req.track_index) {
                continue;
            }
            let Some((rel_path, meta_len)) = file_rel_paths.get(&req.file_index) else {
                done_indices.insert(req.track_index);
                failed_tracks.push((req.track_index, req.track.clone()));
                continue;
            };

            let target_bytes = (*meta_len).max(req.file_size);
            let done_bytes = stats.file_progress.get(req.file_index).copied().unwrap_or(0);
            let is_complete =
                stats.finished || (target_bytes > 0 && done_bytes >= target_bytes);

            if is_complete {
                let src_path = job_dir.join(rel_path);
                let ext = src_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("flac");
                let staged = staging_dir.join(format!(
                    "track_{}_{}.{}",
                    req.track_index,
                    Uuid::new_v4().simple(),
                    ext
                ));
                if let Err(e) = copy_or_move_file(&src_path, &staged).await {
                    tracing::warn!(track = %req.track.title, error = %e, "Failed staging completed torrent track");
                    done_indices.insert(req.track_index);
                    failed_tracks.push((req.track_index, req.track.clone()));
                    continue;
                }

                done_indices.insert(req.track_index);
                emit_track_progress(app, task_id, req.track_index, &req.track.title, 100).await;
                let cur = completed_counter.fetch_add(1, Ordering::SeqCst) + 1;
                emit_task_progress(
                    app,
                    task_id,
                    cur,
                    total_task_tracks,
                    &format!("Downloaded {} of {} tracks", cur, total_task_tracks),
                )
                .await;

                if let Some(ref sender) = track_sender {
                    let _ = sender
                        .send((staged.clone(), req.track.clone(), req.track_index))
                        .await;
                }
                completed.push((staged, req.track.clone(), req.track_index));
            } else if target_bytes > 0 {
                let pct = ((done_bytes.saturating_mul(100)) / target_bytes).min(99) as i32;
                let prev = last_pct.get(&req.track_index).copied().unwrap_or(-1);
                if pct > prev {
                    last_pct.insert(req.track_index, pct);
                    emit_track_progress(app, task_id, req.track_index, &req.track.title, pct).await;
                }
            }
        }

        if done_indices.len() >= requests.len() {
            break;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    for req in requests {
        if !done_indices.contains(&req.track_index) {
            failed_tracks.push((req.track_index, req.track.clone()));
        }
    }

    let _ = session.delete(torrent_id.into(), true).await;
    let _ = tokio::fs::remove_dir_all(&job_dir).await;

    if cancelled {
        return Err("Task cancelled".into());
    }
    if completed.is_empty() {
        if let Some(err) = transfer_failure {
            return Err(err);
        }
    }

    Ok(TorrentTransferResult {
        completed,
        failed_tracks,
    })
}

async fn add_torrent_with_recovery(
    app: &AppHandle,
    task_id: &str,
    cache_root: &std::path::Path,
    job_dir: &std::path::Path,
    locator: &str,
    only_files: &[usize],
    total_task_tracks: usize,
) -> Result<(Arc<Session>, Arc<ManagedTorrent>), String> {
    emit_task_progress(
        app,
        task_id,
        0,
        total_task_tracks,
        "Connecting to torrent peers…",
    )
    .await;

    let cached_bytes = super::inspect::cached_torrent_bytes(locator);
    let session = match get_or_init_session(cache_root.to_path_buf(), false).await {
        Ok(s) => s,
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(job_dir).await;
            return Err(e);
        }
    };

    let primary = try_add_torrent(
        app,
        task_id,
        &session,
        AddTorrent::from_url(locator),
        only_files,
        job_dir,
        total_task_tracks,
        "Connecting to torrent peers",
    )
    .await;

    match primary {
        Ok(handle) => return Ok((session, handle)),
        Err(err) if err == "Task cancelled" => {
            let _ = tokio::fs::remove_dir_all(job_dir).await;
            return Err(err);
        }
        Err(primary_err) => {
            tracing::warn!(
                task_id,
                error = %primary_err,
                "Primary torrent session stalled or failed; resetting session and retrying without DHT"
            );
            // Reset shared session in case librqbit's DHT UDP actor exited (e.g. WSAEMSGSIZE 10040 on Windows).
            reset_session().await;
            emit_task_progress(
                app,
                task_id,
                0,
                total_task_tracks,
                "Retrying torrent via trackers…",
            )
            .await;

            let tracker_session = match get_or_init_session(cache_root.to_path_buf(), true).await {
                Ok(s) => s,
                Err(e) => {
                    let _ = tokio::fs::remove_dir_all(job_dir).await;
                    return Err(e);
                }
            };

            match try_add_torrent(
                app,
                task_id,
                &tracker_session,
                AddTorrent::from_url(locator),
                only_files,
                job_dir,
                total_task_tracks,
                "Retrying via torrent trackers",
            )
            .await
            {
                Ok(handle) => return Ok((tracker_session, handle)),
                Err(err) if err == "Task cancelled" => {
                    let _ = tokio::fs::remove_dir_all(job_dir).await;
                    return Err(err);
                }
                Err(tracker_err) => {
                    if let Some(bytes) = cached_bytes {
                        emit_task_progress(
                            app,
                            task_id,
                            0,
                            total_task_tracks,
                            "Retrying with cached torrent metadata…",
                        )
                        .await;
                        if let Ok(handle) = try_add_torrent(
                            app,
                            task_id,
                            &tracker_session,
                            AddTorrent::from_bytes(bytes),
                            only_files,
                            job_dir,
                            total_task_tracks,
                            "Connecting to cached torrent peers",
                        )
                        .await
                        {
                            return Ok((tracker_session, handle));
                        }
                    }
                    let _ = tokio::fs::remove_dir_all(job_dir).await;
                    return Err(tracker_err);
                }
            }
        }
    }
}

async fn try_add_torrent(
    app: &AppHandle,
    task_id: &str,
    session: &Arc<Session>,
    source: AddTorrent<'_>,
    only_files: &[usize],
    job_dir: &std::path::Path,
    total_task_tracks: usize,
    status_prefix: &str,
) -> Result<Arc<ManagedTorrent>, String> {
    let add_opts = AddTorrentOptions {
        paused: false,
        only_files: Some(only_files.to_vec()),
        overwrite: true,
        output_folder: Some(job_dir.to_string_lossy().to_string()),
        ..Default::default()
    };

    let started = Instant::now();
    let mut last_notice = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut add_fut = std::pin::pin!(session.add_torrent(source, Some(add_opts)));

    let response = loop {
        tokio::select! {
            res = &mut add_fut => {
                break res.map_err(|e| format!("Failed starting torrent download: {e}"))?;
            }
            _ = tick.tick() => {
                if is_task_cancelled(app, task_id).await {
                    return Err("Task cancelled".into());
                }
                let elapsed = started.elapsed();
                if elapsed >= TORRENT_INIT_TIMEOUT {
                    return Err("Timed out connecting to torrent peers or DHT.".into());
                }
                if last_notice.elapsed() >= Duration::from_secs(2) {
                    last_notice = Instant::now();
                    emit_task_progress(
                        app,
                        task_id,
                        0,
                        total_task_tracks,
                        &format!("{status_prefix} ({}s)…", elapsed.as_secs()),
                    )
                    .await;
                }
            }
        }
    };

    response
        .into_handle()
        .ok_or_else(|| "Torrent metadata list-only returned unexpectedly.".to_string())
}

async fn emit_task_progress(
    app: &AppHandle,
    task_id: &str,
    current: usize,
    total: usize,
    message: &str,
) {
    let _ = app.emit(
        "download-task-progress",
        crate::download::types::ProgressEvent {
            task_id: task_id.to_string(),
            current,
            total,
            track_title: message.to_string(),
        },
    );
    if let Some(ref registry) = app.try_state::<crate::download::TaskRegistry>() {
        registry
            .update_task_progress(task_id, current, total, message)
            .await;
    }
}

async fn is_task_cancelled(app: &AppHandle, task_id: &str) -> bool {
    if let Some(registry) = app.try_state::<crate::download::TaskRegistry>() {
        registry.is_cancelled(task_id).await
    } else {
        false
    }
}

async fn emit_track_progress(
    app: &AppHandle,
    task_id: &str,
    track_index: usize,
    title: &str,
    percent: i32,
) {
    let _ = app.emit(
        "download-track-progress",
        serde_json::json!({
            "task_id": task_id,
            "track_index": track_index,
            "track_title": title,
            "percent": percent,
        }),
    );
    if let Some(registry) = app.try_state::<crate::download::TaskRegistry>() {
        registry
            .update_track_progress(task_id, track_index, title, percent)
            .await;
    }
}

async fn copy_or_move_file(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if tokio::fs::rename(src, dst).await.is_ok() {
        return Ok(());
    }
    tokio::fs::copy(src, dst)
        .await
        .map(|_| ())
        .map_err(|e| format!("Failed copying staged torrent audio file: {e}"))
}

pub(crate) fn torrent_cache_root(library_dir: &std::path::Path) -> PathBuf {
    library_dir.join(".temp").join("torrents")
}

pub(crate) fn torrent_staging_dir(cache_root: &std::path::Path, task_id: &str) -> PathBuf {
    let clean: String = task_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    cache_root.join(format!("staged-{}", clean))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn torrent_cache_root_lives_inside_library_temp_dir() {
        let lib = Path::new("/library/music");
        assert_eq!(torrent_cache_root(lib), Path::new("/library/music/.temp/torrents"));
    }

    #[test]
    fn validate_relative_path_rejects_traversal_and_absolute_paths() {
        assert!(validate_relative_path(Path::new("CD1/01 Track.flac")).is_ok());
        assert!(validate_relative_path(Path::new("../secret.flac")).is_err());
        assert!(validate_relative_path(Path::new("")).is_err());
    }

    #[tokio::test]
    async fn staged_audio_file_survives_job_dir_deletion() {
        let root = std::env::temp_dir().join(format!("blacktape-torrent-stage-test-{}", Uuid::new_v4()));
        let job_dir = root.join(format!("job-{}", Uuid::new_v4()));
        let staging_dir = torrent_staging_dir(&root, "task-123/test");
        tokio::fs::create_dir_all(&job_dir).await.unwrap();
        tokio::fs::create_dir_all(&staging_dir).await.unwrap();

        let src = job_dir.join("05 The End.flac");
        let dst = staging_dir.join("05_05 The End.flac");
        tokio::fs::write(&src, b"flac-bytes").await.unwrap();
        copy_or_move_file(&src, &dst).await.unwrap();

        tokio::fs::remove_dir_all(&job_dir).await.unwrap();
        assert!(!job_dir.exists());
        assert!(dst.exists());
        assert_eq!(tokio::fs::read(&dst).await.unwrap(), b"flac-bytes");

        let _ = tokio::fs::remove_dir_all(&root).await;
    }
}
