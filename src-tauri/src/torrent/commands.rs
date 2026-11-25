use super::{inspect, matcher, search};
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};
use tauri::ipc::Channel;
use tauri::{State, WebviewWindow};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct Inspections(Mutex<InspectionJobs>);

#[derive(Default)]
struct InspectionJobs {
    pending: HashMap<Uuid, Option<oneshot::Sender<()>>>,
    cancelled_before_start: VecDeque<Uuid>,
}

impl Inspections {
    fn start(&self, id: Uuid) -> Result<oneshot::Receiver<()>, String> {
        let mut jobs = self
            .0
            .lock()
            .map_err(|_| "Inspection registry unavailable.")?;
        if let Some(index) = jobs
            .cancelled_before_start
            .iter()
            .position(|queued| *queued == id)
        {
            jobs.cancelled_before_start.remove(index);
            return Err("Torrent inspection cancelled.".into());
        }
        if jobs.pending.contains_key(&id) || jobs.pending.len() >= 8 {
            return Err("Too many inspections are pending.".into());
        }
        let (send, receive) = oneshot::channel();
        jobs.pending.insert(id, Some(send));
        Ok(receive)
    }

    fn cancel(&self, id: Uuid) {
        if let Ok(mut jobs) = self.0.lock() {
            if let Some(job) = jobs.pending.get_mut(&id) {
                if let Some(job) = job.take() {
                    let _ = job.send(());
                }
            } else if !jobs.cancelled_before_start.contains(&id) {
                if jobs.cancelled_before_start.len() >= 32 {
                    jobs.cancelled_before_start.pop_front();
                }
                jobs.cancelled_before_start.push_back(id);
            }
        }
    }
}

fn require_main_window(label: &str) -> Result<(), String> {
    if label == "main" {
        Ok(())
    } else {
        Err("Torrent inspection is available from the main window.".into())
    }
}

#[tauri::command]
pub(crate) async fn search_torrent_releases(
    window: WebviewWindow,
    artist: String,
    title: String,
    progress: Channel<search::ProviderReport>,
) -> Result<search::SearchReport, String> {
    require_main_window(window.label())?;
    search::search(&artist, &title, |report| {
        let _ = progress.send(report);
    })
    .await
}

#[tauri::command]
pub(crate) async fn inspect_torrent_release(
    window: WebviewWindow,
    inspections: State<'_, Inspections>,
    request_id: Uuid,
    input: String,
) -> Result<inspect::InspectedRelease, String> {
    require_main_window(window.label())?;
    let cancelled = inspections.start(request_id)?;
    let result = tokio::select! {
        biased;
        _ = cancelled => Err("Torrent inspection cancelled.".into()),
        result = inspect::inspect(&input) => result,
    };
    if let Ok(mut jobs) = inspections.0.lock() {
        jobs.pending.remove(&request_id);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_early_cancel_does_not_start_an_inspection_or_consume_a_job_slot() {
        let inspections = Inspections::default();
        let cancelled = Uuid::new_v4();
        inspections.cancel(cancelled);
        assert!(inspections.start(cancelled).is_err());
        let jobs: Vec<_> = (0..8).map(|_| inspections.start(Uuid::new_v4())).collect();
        assert!(jobs.iter().all(Result::is_ok));
        assert!(inspections.start(Uuid::new_v4()).is_err());
    }
}

#[tauri::command]
pub(crate) async fn resolve_torrent_album(
    window: WebviewWindow,
    inspections: State<'_, Inspections>,
    request_id: Uuid,
    artist: String,
    title: String,
    expected: Vec<matcher::ExpectedTrackInput>,
    progress: Channel<matcher::TorrentResolveEvent>,
) -> Result<(Vec<matcher::TorrentTierResolution>, Vec<String>), String> {
    require_main_window(window.label())?;
    let cancelled = inspections.start(request_id)?;
    let result = tokio::select! {
        biased;
        _ = cancelled => Err("Torrent resolution cancelled.".into()),
        res = matcher::resolve_album_torrents(&artist, &title, &expected, |event| {
            let _ = progress.send(event);
        }) => res,
    };
    if let Ok(mut jobs) = inspections.0.lock() {
        jobs.pending.remove(&request_id);
    }
    result
}

#[tauri::command]
pub(crate) fn cancel_torrent_inspection(
    window: WebviewWindow,
    inspections: State<'_, Inspections>,
    request_id: Uuid,
) -> Result<(), String> {
    require_main_window(window.label())?;
    inspections.cancel(request_id);
    Ok(())
}
