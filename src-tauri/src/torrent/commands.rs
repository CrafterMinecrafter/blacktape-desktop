use super::{inspect, matcher, search};
use crate::db::db::Database;
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State, WebviewWindow};
use tokio::sync::oneshot;
use uuid::Uuid;

type DbLock = tokio::sync::Mutex<Database>;
const DISABLED_INDEXERS_KEY: &str = "disabled_torrent_indexers";
const RUTRACKER_ENABLED_KEY: &str = "rutracker_enabled";

#[derive(Serialize)]
pub(crate) struct TorrentIndexer {
    id: search::Provider,
    name: &'static str,
    enabled: bool,
}

#[derive(Serialize)]
pub(crate) struct TorrentIndexerTest {
    hits: usize,
    error: Option<String>,
}

async fn disabled_indexers(db: &Database) -> Result<Vec<search::Provider>, String> {
    let value = db.get_app_setting(DISABLED_INDEXERS_KEY).await
        .map_err(|_| "Could not load torrent indexer settings.")?;
    match value {
        Some(value) => serde_json::from_str(&value).map_err(|_| "Could not read torrent indexer settings.".into()),
        None => Ok(Vec::new()),
    }
}

async fn indexers(db: &Database, disabled: &[search::Provider]) -> Result<Vec<TorrentIndexer>, String> {
    let rutracker = db.get_app_setting(RUTRACKER_ENABLED_KEY).await
        .map_err(|_| "Could not load RuTracker settings.")?.as_deref() == Some("true");
    Ok(search::Provider::ALL.into_iter().chain([search::Provider::RuTracker]).map(|id| TorrentIndexer {
        id,
        name: id.name(),
        enabled: if id == search::Provider::RuTracker { rutracker } else { !disabled.contains(&id) },
    }).collect())
}

async fn enabled_indexers(db: &DbLock) -> Result<Vec<search::Provider>, String> {
    let db = db.lock().await;
    let disabled = disabled_indexers(&db).await?;
    Ok(indexers(&db, &disabled).await?.into_iter().filter(|indexer| indexer.enabled).map(|indexer| indexer.id).collect())
}

async fn save_indexer(
    db: &Database,
    id: search::Provider,
    enabled: bool,
) -> Result<Vec<TorrentIndexer>, String> {
    let mut disabled = disabled_indexers(db).await?;
    if id == search::Provider::RuTracker {
        db.set_app_setting(RUTRACKER_ENABLED_KEY, if enabled { "true" } else { "false" }).await
            .map_err(|_| "Could not save RuTracker settings.")?;
        return indexers(db, &disabled).await;
    }
    if enabled {
        disabled.retain(|provider| *provider != id);
    } else if !disabled.contains(&id) {
        disabled.push(id);
    }
    let value = serde_json::to_string(&disabled).map_err(|_| "Could not encode torrent indexer settings.")?;
    db.set_app_setting(DISABLED_INDEXERS_KEY, &value).await
        .map_err(|_| "Could not save torrent indexer settings.")?;
    indexers(db, &disabled).await
}

fn require_settings_window(label: &str) -> Result<(), String> {
    if label == "settings" {
        Ok(())
    } else {
        Err("Torrent indexers are configured from the Settings window.".into())
    }
}

#[tauri::command]
pub(crate) async fn list_torrent_indexers(
    window: WebviewWindow,
    db: State<'_, DbLock>,
) -> Result<Vec<TorrentIndexer>, String> {
    require_settings_window(window.label())?;
    let db = db.lock().await;
    indexers(&db, &disabled_indexers(&db).await?).await
}

#[tauri::command]
pub(crate) async fn set_torrent_indexer_enabled(
    window: WebviewWindow,
    db: State<'_, DbLock>,
    id: search::Provider,
    enabled: bool,
) -> Result<Vec<TorrentIndexer>, String> {
    require_settings_window(window.label())?;
    let updated = {
        let db = db.lock().await;
        save_indexer(&db, id, enabled).await?
    };
    let _ = window.emit_to("main", "torrent-indexers-changed", ());
    Ok(updated)
}

#[tauri::command]
pub(crate) async fn test_torrent_indexer(
    window: WebviewWindow,
    id: search::Provider,
) -> Result<TorrentIndexerTest, String> {
    require_settings_window(window.label())?;
    let report = search::test_provider(id, Some(window.app_handle())).await?;
    if id == search::Provider::RuTracker && report.error.is_none() {
        let _ = window.emit_to("main", "torrent-indexers-changed", ());
    }
    Ok(TorrentIndexerTest { hits: report.hits.len(), error: report.error })
}

#[tauri::command]
pub(crate) async fn open_rutracker_login(window: WebviewWindow) -> Result<(), String> {
    require_settings_window(window.label())?;
    super::rutracker::open_login(window.app_handle()).await
}

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
    db: State<'_, DbLock>,
    artist: String,
    title: String,
    progress: Channel<search::ProviderReport>,
) -> Result<search::SearchReport, String> {
    require_main_window(window.label())?;
    let enabled = enabled_indexers(&db).await?;
    search::search_with_app(Some(window.app_handle()), &artist, &title, false, &enabled, |report| {
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
        result = inspect::inspect_with_app(Some(window.app_handle()), &input) => result,
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

    #[tokio::test]
    async fn indexers_default_to_enabled_and_saved_toggles_control_search() {
        use search::Provider::{ApiBay, Archive, Knaben, Rutor, SolidTorrents};
        let db = DbLock::new(Database::new(":memory:").await);
        assert_eq!(enabled_indexers(&db).await.unwrap(), [Archive, Knaben, ApiBay, SolidTorrents, Rutor]);
        {
            let db = db.lock().await;
            save_indexer(&db, Archive, false).await.unwrap();
            save_indexer(&db, Archive, false).await.unwrap();
            save_indexer(&db, ApiBay, false).await.unwrap();
            save_indexer(&db, Rutor, false).await.unwrap();
            let state = save_indexer(&db, SolidTorrents, false).await.unwrap();
            assert_eq!(disabled_indexers(&db).await.unwrap(), [Archive, ApiBay, Rutor, SolidTorrents]);
            assert_eq!(state.iter().filter(|indexer| indexer.enabled).count(), 1);
        }
        assert_eq!(enabled_indexers(&db).await.unwrap(), [Knaben]);
        {
            let db = db.lock().await;
            save_indexer(&db, Knaben, false).await.unwrap();
        }
        let enabled = enabled_indexers(&db).await.unwrap();
        let search = search::search_for_release("The Beatles", "Abbey Road", false, &enabled, |_| {
            panic!("disabled indexer was queried");
        }).await.unwrap();
        assert!(search.providers.is_empty());
        assert!(search.hits.is_empty());
        let (resolved, _) = matcher::resolve_with_indexers(None, "The Beatles", "Abbey Road", &[], &enabled, |_| {})
            .await.unwrap();
        assert!(resolved.is_empty());
        {
            let db = db.lock().await;
            save_indexer(&db, Archive, true).await.unwrap();
        }
        assert_eq!(enabled_indexers(&db).await.unwrap(), [Archive]);
    }

    #[tokio::test]
    async fn rutracker_is_opt_in_and_leaves_public_defaults_intact() {
        use search::Provider::RuTracker;
        let db = DbLock::new(Database::new(":memory:").await);
        assert!(!enabled_indexers(&db).await.unwrap().contains(&RuTracker));
        {
            let db = db.lock().await;
            let rows = save_indexer(&db, RuTracker, true).await.unwrap();
            assert!(rows.iter().find(|row| row.id == RuTracker).unwrap().enabled);
            assert!(rows.iter().all(|row| row.enabled));
        }
        assert!(enabled_indexers(&db).await.unwrap().contains(&RuTracker));
        {
            let db = db.lock().await;
            save_indexer(&db, RuTracker, false).await.unwrap();
        }
        assert_eq!(enabled_indexers(&db).await.unwrap(), search::Provider::ALL);
    }

    #[test]
    fn indexer_settings_and_torrent_search_use_separate_windows() {
        assert!(require_settings_window("settings").is_ok());
        assert!(require_settings_window("main").is_err());
        assert!(require_settings_window("youtube-login").is_err());
        assert!(require_main_window("settings").is_err());
    }
}

#[tauri::command]
pub(crate) async fn resolve_torrent_album(
    window: WebviewWindow,
    inspections: State<'_, Inspections>,
    db: State<'_, DbLock>,
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
        res = async {
            let enabled = enabled_indexers(&db).await?;
            matcher::resolve_with_indexers(Some(window.app_handle()), &artist, &title, &expected, &enabled, |event| {
                let _ = progress.send(event);
            }).await
        } => res,
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
