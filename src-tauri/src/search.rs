use crate::archive::{is_archive_name, is_log_name};
#[cfg(windows)]
use crate::ntfs::{
    ipc::{enumerate_mft_via_service, query_usn_via_service, read_usn_via_service, ServiceFailure},
    resolve_mft_files_in_batches, resolve_mft_files_in_batches_retain, MftRecord, UsnJournalInfo,
    FILE_ATTRIBUTE_DIRECTORY,
};
use crate::search_index::{SearchIndex, SearchIndexEntry};
use crate::search_query_store::{
    previous_path as query_index_previous_path,
    recover_directories as recover_query_index_directories, retry_fs as retry_query_index_fs,
    staging_path as query_index_staging_path,
};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
#[cfg(windows)]
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{
    sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Emitter;

const SCHEMA_VERSION: i64 = 9;
const SCAN_WRITE_BATCH: usize = 8_192;
#[cfg(windows)]
const NTFS_RESOLVE_BATCH: usize = 2_048;
const EVENT_BATCH: usize = 512;
const EVENT_QUEUE_CAPACITY: usize = 4096;
const EVENT_HANDOFF_MAX_BATCHES: usize = 4;
const EVENT_HANDOFF_MAX_DURATION: Duration = Duration::from_millis(500);
const QUERY_LIMIT_MAX: u32 = 500;
const METADATA_WORKERS_MAX: usize = 4;
#[cfg(windows)]
const MAX_USN_REPLAY_RECORDS: usize = 1_000_000;
#[cfg(windows)]
const PERSISTENCE_USN_REPLAY_MAX_DURATION: Duration = Duration::from_secs(30);
#[cfg(windows)]
// A synchronous named-pipe read has no client-side cancellation before its first batch.
// During a full rebuild the watcher already owns reconciliation, so only a strictly
// unchanged journal can be finalized without risking an unbounded persistence tail.
const PERSISTENCE_USN_REPLAY_MAX_RANGE: i64 = 0;
#[cfg(windows)]
const NTFS_VOLUME_WORKERS_MAX: usize = 4;

const CREATE_FTS_TRIGGERS: &str =
    "CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
       INSERT INTO files_fts(rowid, name, path) VALUES (new.rowid, new.name, new.path);
     END;
     CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
       INSERT INTO files_fts(files_fts, rowid, name, path)
       VALUES('delete', old.rowid, old.name, old.path);
     END;
     CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE ON files BEGIN
       INSERT INTO files_fts(files_fts, rowid, name, path)
       VALUES('delete', old.rowid, old.name, old.path);
       INSERT INTO files_fts(rowid, name, path) VALUES (new.rowid, new.name, new.path);
     END;";
const DROP_FTS_TRIGGERS: &str = "DROP TRIGGER IF EXISTS files_ai;
     DROP TRIGGER IF EXISTS files_ad;
     DROP TRIGGER IF EXISTS files_au;";
const CREATE_QUERY_CHANGE_TRIGGERS: &str =
    "CREATE TRIGGER IF NOT EXISTS files_q_ai AFTER INSERT ON files BEGIN
       INSERT INTO search_index_changes(path, operation) VALUES(new.path, 1)
       ON CONFLICT(path) DO UPDATE SET operation=1;
     END;
     CREATE TRIGGER IF NOT EXISTS files_q_ad AFTER DELETE ON files BEGIN
       INSERT INTO search_index_changes(path, operation) VALUES(old.path, 0)
       ON CONFLICT(path) DO UPDATE SET operation=0;
     END;
     CREATE TRIGGER IF NOT EXISTS files_q_au AFTER UPDATE ON files BEGIN
       INSERT INTO search_index_changes(path, operation) VALUES(old.path, 0)
       ON CONFLICT(path) DO UPDATE SET operation=0;
       INSERT INTO search_index_changes(path, operation) VALUES(new.path, 1)
       ON CONFLICT(path) DO UPDATE SET operation=1;
     END;";
const DROP_QUERY_CHANGE_TRIGGERS: &str = "DROP TRIGGER IF EXISTS files_q_ai;
     DROP TRIGGER IF EXISTS files_q_ad;
     DROP TRIGGER IF EXISTS files_q_au;";
const CREATE_FTS_TABLE: &str = "CREATE VIRTUAL TABLE files_fts USING fts5(
       name, path, content='files', content_rowid='rowid',
       tokenize='trigram', detail='none', columnsize=0
     );";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchConfig {
    #[serde(default = "search_config_version")]
    pub version: u32,
    pub enabled: bool,
    pub roots: Vec<String>,
    pub exclusions: Vec<String>,
}

const fn search_config_version() -> u32 {
    1
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            version: search_config_version(),
            // Local file search is a core capability.  An explicit value in an
            // existing config still wins; this only affects first launch.
            enabled: true,
            roots: local_fixed_roots(),
            exclusions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchFeatureState {
    pub current_enabled: bool,
    pub next_launch_enabled: bool,
}

#[derive(Clone)]
pub struct SearchPreferenceStore {
    config_path: PathBuf,
    config: Arc<Mutex<SearchConfig>>,
}

impl SearchPreferenceStore {
    pub fn new(data_dir: PathBuf) -> Self {
        let config_path = data_dir.join("file-search.json");
        let config = read_config(&config_path).unwrap_or_default();
        Self {
            config_path,
            config: Arc::new(Mutex::new(config)),
        }
    }

    pub fn config(&self) -> SearchConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn feature_state(&self, current_enabled: bool) -> SearchFeatureState {
        SearchFeatureState {
            current_enabled,
            next_launch_enabled: self.config.lock().unwrap().enabled,
        }
    }

    pub fn set_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        let mut config = self.config.lock().unwrap();
        let previous = config.enabled;
        config.enabled = enabled;
        config.version = search_config_version();
        if let Err(error) = write_config(&self.config_path, &config) {
            config.enabled = previous;
            return Err(error);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatus {
    pub phase: String,
    pub scanned_files: u64,
    pub skipped_directories: u64,
    pub indexed_files: u64,
    pub index_bytes: u64,
    pub roots: Vec<String>,
    pub exclusions: Vec<String>,
    pub providers: Vec<SearchProviderStatus>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchProviderStatus {
    pub root: String,
    pub provider: String,
    pub phase: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub discovered_records: u64,
    #[serde(default)]
    pub searchable_files: u64,
    #[serde(default)]
    pub started_ms: Option<u64>,
    #[serde(default)]
    pub elapsed_ms: u64,
    #[serde(default)]
    pub stage_started_ms: Option<u64>,
    #[serde(default)]
    pub stage_elapsed_ms: u64,
    #[serde(default)]
    pub completed_ms: Option<u64>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexScopeSnapshot {
    pub scope_key: String,
    pub provider: String,
    pub phase: String,
    pub discovered_records: u64,
    pub searchable_files: u64,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexOperationSnapshot {
    pub operation_id: String,
    pub generation: u64,
    pub started_ms: u64,
    pub query_ready_ms: Option<u64>,
    pub persistence_completed_ms: Option<u64>,
    pub event_handoff_completed_ms: Option<u64>,
    pub converged_ms: Option<u64>,
    pub final_phase: String,
    pub error: Option<String>,
    pub scopes: Vec<IndexScopeSnapshot>,
}

pub(crate) trait SearchStatusSink: Clone + Send + Sync + 'static {
    fn emit_search_status(&self, status: SearchStatus);
}

impl<R: tauri::Runtime> SearchStatusSink for tauri::AppHandle<R> {
    fn emit_search_status(&self, status: SearchStatus) {
        let _ = self.emit("file-search-status", status);
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
struct RecordingSearchStatusSink(Arc<Mutex<Vec<SearchStatus>>>);

#[cfg(test)]
impl SearchStatusSink for RecordingSearchStatusSink {
    fn emit_search_status(&self, status: SearchStatus) {
        self.0.lock().unwrap().push(status);
    }
}

#[cfg(all(test, windows))]
#[derive(Clone, Copy)]
struct NoopSearchStatusSink;

#[cfg(all(test, windows))]
impl SearchStatusSink for NoopSearchStatusSink {
    fn emit_search_status(&self, _status: SearchStatus) {}
}

impl SearchStatus {
    fn disabled(config: &SearchConfig) -> Self {
        Self {
            phase: "disabled".into(),
            scanned_files: 0,
            skipped_directories: 0,
            indexed_files: 0,
            index_bytes: 0,
            roots: config.roots.clone(),
            exclusions: config.exclusions.clone(),
            providers: planned_provider_statuses(&config.roots),
            error: None,
        }
    }

    pub(crate) fn initializing(config: &SearchConfig) -> Self {
        let mut status = Self::disabled(config);
        if config.enabled {
            status.phase = "scanning".into();
            if status.roots.is_empty() {
                status.roots = local_fixed_roots();
                status.providers = planned_provider_statuses(&status.roots);
            }
        }
        status
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultItem {
    pub path: String,
    pub name: String,
    pub parent: String,
    pub kind: String,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub readable: bool,
    pub content_type: String,
    pub is_log: bool,
    pub is_archive: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub items: Vec<SearchResultItem>,
    pub total: u64,
    pub partial: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
struct IndexedFile {
    path: String,
    name: String,
    root: String,
    size: u64,
    modified_ms: Option<u64>,
    is_log: bool,
    is_archive: bool,
    file_id: Option<[u8; 16]>,
    parent_id: Option<[u8; 16]>,
}

pub struct FileSearchManager {
    db_path: PathBuf,
    config_path: PathBuf,
    query_index_path: PathBuf,
    config: Arc<Mutex<SearchConfig>>,
    status: Mutex<SearchStatus>,
    generation: AtomicU64,
    cancel: AtomicBool,
    watcher: Mutex<Option<RecommendedWatcher>>,
    event_sender: Mutex<Option<SyncSender<Event>>>,
    event_dirty: Arc<AtomicBool>,
    query_index: Mutex<Option<SearchIndex>>,
    staged_query_index: Mutex<Option<SearchIndex>>,
    query_index_ready: AtomicBool,
    query_index_bulk: AtomicBool,
    query_index_staged: AtomicBool,
    persistence_recovery: AtomicBool,
    corruption_recovery: AtomicBool,
    operation: Mutex<()>,
    operation_snapshot: Mutex<Option<IndexOperationSnapshot>>,
    active_queries: AtomicU64,
}

struct QueryActivityGuard<'a>(&'a AtomicU64);

impl Drop for QueryActivityGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl FileSearchManager {
    #[cfg(test)]
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        let preferences = SearchPreferenceStore::new(data_dir.clone());
        Self::new_with_preferences(data_dir, &preferences)
    }

    pub fn new_with_preferences(
        data_dir: PathBuf,
        preferences: &SearchPreferenceStore,
    ) -> Arc<Self> {
        let _ = fs::create_dir_all(&data_dir);
        let config_path = preferences.config_path.clone();
        let query_index_path = data_dir.join("file-search-orange-gpl-v1");
        let _ = recover_query_index_directories(&query_index_path);
        let config_state = preferences.config.clone();
        let config = config_state.lock().unwrap().clone();
        let mut status = SearchStatus::disabled(&config);
        if config.enabled && data_dir.join("file-search.sqlite3").is_file() {
            status.phase = "ready".into();
        }
        let query_index = SearchIndex::open(&query_index_path);
        if let Err(error) = &query_index {
            status.error = Some(format!("Tantivy query index: {error}"));
        }
        let query_documents_at_start = query_index.as_ref().ok().map(SearchIndex::num_docs);
        let database_path = data_dir.join("file-search.sqlite3");
        let database_state =
            initialize_database_with_query(&database_path, query_documents_at_start).or_else(
                |error| {
                    if is_database_corruption(&error) {
                        quarantine_database(&database_path)?;
                        initialize_database_with_query(&database_path, None)
                    } else {
                        Err(error)
                    }
                },
            );
        let manager = Arc::new(Self {
            db_path: data_dir.join("file-search.sqlite3"),
            config_path,
            query_index_path,
            config: config_state,
            status: Mutex::new(status),
            generation: AtomicU64::new(0),
            cancel: AtomicBool::new(false),
            watcher: Mutex::new(None),
            event_sender: Mutex::new(None),
            event_dirty: Arc::new(AtomicBool::new(false)),
            query_index: Mutex::new(query_index.ok()),
            staged_query_index: Mutex::new(None),
            query_index_ready: AtomicBool::new(false),
            query_index_bulk: AtomicBool::new(false),
            query_index_staged: AtomicBool::new(false),
            persistence_recovery: AtomicBool::new(false),
            corruption_recovery: AtomicBool::new(false),
            operation: Mutex::new(()),
            operation_snapshot: Mutex::new(None),
            active_queries: AtomicU64::new(0),
        });
        match database_state {
            Err(error) => manager.status.lock().unwrap().error = Some(error.to_string()),
            Ok(database_state) => {
                manager.refresh_counts();
                let database_documents = manager.status.lock().unwrap().indexed_files;
                let query_documents = manager
                    .query_index
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(SearchIndex::num_docs);
                let query_ready = query_documents == Some(database_documents)
                    || (database_state.query_snapshot_complete
                        && query_documents.is_some_and(|documents| documents > 0));
                manager
                    .query_index_ready
                    .store(query_ready, Ordering::Release);
                manager
                    .persistence_recovery
                    .store(database_state.persistence_incomplete, Ordering::Release);
                if query_ready && manager.config.lock().unwrap().enabled {
                    let mut status = manager.status.lock().unwrap();
                    status.phase = "ready".into();
                    if database_state.query_snapshot_complete {
                        if let Some(query_documents) = query_documents {
                            status.indexed_files = query_documents;
                        }
                    }
                }
            }
        }
        manager
    }

    pub fn config(&self) -> SearchConfig {
        self.config.lock().unwrap().clone()
    }

    fn runtime_config(&self) -> SearchConfig {
        let mut config = self.config();
        if let Some(data_dir) = self.db_path.parent() {
            config
                .exclusions
                .push(data_dir.to_string_lossy().into_owned());
        }
        config.exclusions = normalize_unique_paths(config.exclusions);
        config
    }

    pub fn status(&self) -> SearchStatus {
        self.refresh_counts();
        let mut status = self.status.lock().unwrap();
        refresh_provider_elapsed(&mut status);
        status.clone()
    }

    pub fn start<S: SearchStatusSink>(
        self: &Arc<Self>,
        app: S,
        rebuild: bool,
    ) -> anyhow::Result<()> {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let now_ms = system_time_ms(SystemTime::now()).unwrap_or(0);
        let scopes = self
            .runtime_config()
            .roots
            .into_iter()
            .map(|scope_key| IndexScopeSnapshot {
                scope_key,
                provider: "pending".into(),
                phase: "pending".into(),
                discovered_records: 0,
                searchable_files: 0,
                elapsed_ms: 0,
                error: None,
            })
            .collect();
        *self.operation_snapshot.lock().unwrap() = Some(IndexOperationSnapshot {
            operation_id: format!("search-{generation}"),
            generation,
            started_ms: now_ms,
            query_ready_ms: None,
            persistence_completed_ms: None,
            event_handoff_completed_ms: None,
            converged_ms: None,
            final_phase: "scanning".into(),
            error: None,
            scopes,
        });
        self.cancel.store(false, Ordering::SeqCst);
        self.stop_watcher();
        {
            let mut config = self.config.lock().unwrap();
            if config.roots.is_empty() {
                config.roots = local_fixed_roots();
            }
            write_config(&self.config_path, &config)?;
            let mut status = self.status.lock().unwrap();
            status.phase = "scanning".into();
            status.scanned_files = 0;
            if rebuild {
                status.indexed_files = 0;
            }
            status.skipped_directories = 0;
            status.roots = config.roots.clone();
            status.exclusions = config.exclusions.clone();
            status.providers = planned_provider_statuses(&config.roots);
            status.error = None;
        }
        self.emit_status(&app);
        let config = self.runtime_config();

        let manager = Arc::clone(self);
        std::thread::spawn(move || {
            let operation_started = std::time::Instant::now();
            let _operation = manager
                .operation
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if manager.is_cancelled(generation) {
                return;
            }
            let receiver = match manager.install_watcher(&config) {
                Ok(receiver) => receiver,
                Err(error) => {
                    manager.finish_with_error(&app, generation, error);
                    return;
                }
            };
            if let Err(error) = prepare_bulk_index(&manager.db_path, rebuild) {
                manager.stop_watcher();
                manager.finish_with_error(&app, generation, error);
                return;
            }
            if rebuild {
                if let Err(error) = manager.begin_query_index_bulk() {
                    manager.stop_watcher();
                    manager.finish_with_error(&app, generation, error);
                    return;
                }
                manager.query_index_ready.store(true, Ordering::Release);
            }
            match scan_with_providers(&manager, &app, generation, &config) {
                Ok(outcome) if !manager.is_cancelled(generation) => {
                    if let Err(error) = manager.finish_query_index_bulk() {
                        manager.stop_watcher();
                        manager.finish_with_error(&app, generation, error);
                        return;
                    }
                    #[cfg(windows)]
                    if let Err(error) = mark_query_snapshot_complete(&manager.db_path) {
                        manager.stop_watcher();
                        manager.finish_with_error(&app, generation, error);
                        return;
                    }
                    let has_deferred_persistence = outcome.has_deferred_persistence();
                    manager
                        .persistence_recovery
                        .store(has_deferred_persistence, Ordering::Release);
                    #[cfg(windows)]
                    for job in &outcome.ntfs_finalize_jobs {
                        set_provider_stage(
                            &manager,
                            &app,
                            &job.root,
                            "windowsNtfs",
                            "ready",
                            "persisting",
                            None,
                        );
                    }
                    {
                        let mut status = manager.status.lock().unwrap();
                        status.phase = "ready".into();
                        status.error = None;
                    }
                    manager.corruption_recovery.store(false, Ordering::Release);
                    if let Some(snapshot) = manager.operation_snapshot.lock().unwrap().as_mut() {
                        let now = system_time_ms(SystemTime::now()).unwrap_or(0);
                        snapshot.query_ready_ms = Some(now);
                        snapshot.final_phase = "ready".into();
                        snapshot.converged_ms = Some(now);
                    }
                    manager.emit_status(&app);
                    eprintln!(
                        "[search-index] generation={generation} query-ready elapsed_ms={}",
                        operation_started.elapsed().as_millis()
                    );
                    #[cfg(windows)]
                    if !outcome.ntfs_finalize_jobs.is_empty() {
                        if let Err(error) = begin_ntfs_nodes_bulk(&manager.db_path) {
                            manager.stop_watcher();
                            manager.finish_with_error(&app, generation, error);
                            return;
                        }
                    }
                    #[cfg(windows)]
                    let mut persisted_ntfs_roots = Vec::new();
                    #[cfg(windows)]
                    for job in outcome.ntfs_finalize_jobs {
                        let root = job.root.clone();
                        if let Err(error) =
                            persist_and_finalize_ntfs_volume(&manager.db_path, job, false)
                        {
                            let _ = finish_ntfs_nodes_bulk(&manager.db_path);
                            set_provider_stage(
                                &manager,
                                &app,
                                &root,
                                "windowsNtfs",
                                "error",
                                "error",
                                Some(error.to_string()),
                            );
                            manager.stop_watcher();
                            manager.finish_with_error(&app, generation, error);
                            return;
                        }
                        persisted_ntfs_roots.push(root);
                    }
                    #[cfg(windows)]
                    if let Err(error) = finish_ntfs_nodes_bulk(&manager.db_path) {
                        manager.stop_watcher();
                        manager.finish_with_error(&app, generation, error);
                        return;
                    }
                    #[cfg(windows)]
                    for root in persisted_ntfs_roots {
                        set_provider_status(&manager, &app, &root, "windowsNtfs", "ready", None);
                    }
                    if let Err(error) = finish_bulk_index(&manager.db_path) {
                        manager.stop_watcher();
                        manager.finish_with_error(&app, generation, error);
                        return;
                    }
                    if let Some(snapshot) = manager.operation_snapshot.lock().unwrap().as_mut() {
                        let now = system_time_ms(SystemTime::now()).unwrap_or(0);
                        snapshot.persistence_completed_ms = Some(now);
                        snapshot.event_handoff_completed_ms = Some(now);
                        snapshot.converged_ms = Some(now);
                    }
                    manager.persistence_recovery.store(false, Ordering::Release);
                    if manager.is_cancelled(generation) {
                        manager.stop_watcher();
                        return;
                    }
                    let handoff_paths = collect_event_paths_bounded(&receiver);
                    manager.spawn_event_worker(
                        app.clone(),
                        generation,
                        config,
                        receiver,
                        handoff_paths,
                    );
                    manager.refresh_counts();
                    manager.emit_status(&app);
                }
                Ok(_) => manager.stop_watcher(),
                Err(error) => {
                    manager.stop_watcher();
                    manager.finish_with_error(&app, generation, error);
                }
            }
        });
        Ok(())
    }

    pub fn resume_or_watch<S: SearchStatusSink>(self: &Arc<Self>, app: S) -> anyhow::Result<()> {
        let config = self.runtime_config();
        let has_persisted_files = self.status.lock().unwrap().indexed_files > 0;
        let has_search_snapshot = self.query_index_ready.load(Ordering::Acquire);
        if self.db_path.is_file() && (has_persisted_files || has_search_snapshot) {
            let receiver = self.install_watcher(&config)?;
            #[cfg(windows)]
            {
                let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
                self.cancel.store(false, Ordering::SeqCst);
                let recover_persistence = self.persistence_recovery.load(Ordering::Acquire);
                self.status.lock().unwrap().phase = "scanning".into();
                let manager = Arc::clone(self);
                std::thread::spawn(move || {
                    let _operation = manager
                        .operation
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    if manager.is_cancelled(generation) {
                        return;
                    }
                    if let Err(error) = manager.ensure_query_index_matches_database() {
                        manager.finish_with_error(&app, generation, error);
                        return;
                    }
                    let ntfs_roots = config
                        .roots
                        .iter()
                        .filter_map(|root| {
                            ntfs_volume_letter(root).map(|volume| (root.clone(), volume))
                        })
                        .collect::<Vec<_>>();
                    let work = |root: String, volume| {
                        set_provider_stage(
                            &manager,
                            &app,
                            &root,
                            "windowsNtfs",
                            "scanning",
                            "connecting",
                            None,
                        );
                        if recover_persistence {
                            enumerate_ntfs_volume_snapshot(
                                &manager, &app, generation, &config, &root, volume,
                            )
                            .map(NtfsResumeJob::Rebuild)
                        } else {
                            set_provider_stage(
                                &manager,
                                &app,
                                &root,
                                "windowsNtfs",
                                "scanning",
                                "readingUsn",
                                None,
                            );
                            prepare_ntfs_catch_up(
                                &manager.db_path,
                                &root,
                                volume,
                                &config.exclusions,
                            )
                            .map(NtfsResumeJob::CatchUp)
                            .or_else(|error| {
                                eprintln!(
                                    "[search-index] root={root} strategy=full-rebuild reason={error}"
                                );
                                enumerate_ntfs_volume_snapshot(
                                    &manager, &app, generation, &config, &root, volume,
                                )
                                .map(NtfsResumeJob::Rebuild)
                            })
                        }
                    };
                    let mut fallback_roots = Vec::new();
                    let resume_result = run_ntfs_volume_tasks(
                        ntfs_roots,
                        &work,
                        &|| manager.is_cancelled(generation),
                        |root, result| {
                            let applied = match result {
                                Ok(NtfsResumeJob::CatchUp(job)) => {
                                    set_provider_stage(
                                        &manager,
                                        &app,
                                        &root,
                                        "windowsNtfs",
                                        "merging",
                                        "merging",
                                        None,
                                    );
                                    apply_ntfs_catch_up(&manager.db_path, job)
                                }
                                Ok(NtfsResumeJob::Rebuild(snapshot)) => {
                                    set_provider_stage(
                                        &manager,
                                        &app,
                                        &root,
                                        "windowsNtfs",
                                        "merging",
                                        "resolvingPaths",
                                        None,
                                    );
                                    index_ntfs_volume_paths(&manager, &app, generation, snapshot)
                                        .and_then(|job| {
                                            set_provider_stage(
                                                &manager,
                                                &app,
                                                &root,
                                                "windowsNtfs",
                                                "merging",
                                                "persisting",
                                                None,
                                            );
                                            persist_and_finalize_ntfs_volume(
                                                &manager.db_path,
                                                job,
                                                true,
                                            )
                                        })
                                }
                                Err(error) => Err(error),
                            };
                            if let Err(error) = applied {
                                if !manager.is_cancelled(generation) {
                                    match compatible_provider_fallback_reason(&root, &error) {
                                        Ok(Some(reason)) => {
                                            eprintln!(
                                                "[search-index] root={root} strategy=folder-fallback reason={error}"
                                            );
                                            set_provider_stage(
                                                &manager,
                                                &app,
                                                &root,
                                                "folderScan",
                                                "fallback",
                                                "fallback",
                                                Some(reason),
                                            );
                                            fallback_roots.push(root);
                                            return Ok(());
                                        }
                                        Ok(None) => {}
                                        Err(fallback_error) => {
                                            let combined = anyhow::anyhow!(
                                                "{error}; 自动降级兼容 provider 失败: {fallback_error}"
                                            );
                                            set_provider_status(
                                                &manager,
                                                &app,
                                                &root,
                                                "windowsNtfs",
                                                "error",
                                                Some(combined.to_string()),
                                            );
                                            return Err(combined);
                                        }
                                    }
                                }
                                set_provider_status(
                                    &manager,
                                    &app,
                                    &root,
                                    "windowsNtfs",
                                    "error",
                                    Some(error.to_string()),
                                );
                                return Err(error);
                            }
                            if !recover_persistence {
                                manager.drain_query_index_changes()?;
                            }
                            set_provider_searchable_files(
                                &manager,
                                &app,
                                &root,
                                indexed_root_count(&manager.db_path, &root),
                            );
                            set_provider_status(
                                &manager,
                                &app,
                                &root,
                                "windowsNtfs",
                                "ready",
                                None,
                            );
                            Ok(())
                        },
                    );
                    let completed = match resume_result {
                        Ok(completed) => completed,
                        Err(error) => {
                            manager.finish_with_error(&app, generation, error);
                            return;
                        }
                    };
                    if !completed {
                        manager.stop_watcher();
                        return;
                    }
                    if manager.is_cancelled(generation) {
                        manager.stop_watcher();
                        return;
                    }
                    if !fallback_roots.is_empty() {
                        for root in &fallback_roots {
                            if let Err(error) =
                                clear_root_for_compatible_provider(&manager.db_path, root)
                            {
                                manager.finish_with_error(&app, generation, error);
                                return;
                            }
                        }
                        if let Err(error) =
                            scan_folder_roots(&manager, &app, generation, &config, &fallback_roots)
                                .and_then(|_| manager.drain_query_index_changes())
                        {
                            manager.finish_with_error(&app, generation, error);
                            return;
                        }
                    }
                    if recover_persistence {
                        if let Err(error) = finish_bulk_index(&manager.db_path) {
                            manager.finish_with_error(&app, generation, error);
                            return;
                        }
                        manager.persistence_recovery.store(false, Ordering::Release);
                    }
                    let handoff_paths = collect_event_paths_bounded(&receiver);
                    if manager.is_cancelled(generation) {
                        manager.stop_watcher();
                        return;
                    }
                    manager.spawn_event_worker(
                        app.clone(),
                        generation,
                        config,
                        receiver,
                        handoff_paths,
                    );
                    manager.status.lock().unwrap().phase = "ready".into();
                    manager.refresh_counts();
                    manager.emit_status(&app);
                });
                Ok(())
            }
            #[cfg(not(windows))]
            {
                let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
                self.cancel.store(false, Ordering::SeqCst);
                self.status.lock().unwrap().phase = "scanning".into();
                let manager = Arc::clone(self);
                std::thread::spawn(move || {
                    let _operation = manager
                        .operation
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    if manager.is_cancelled(generation) {
                        return;
                    }
                    if let Err(error) = manager.ensure_query_index_matches_database() {
                        manager.finish_with_error(&app, generation, error);
                        return;
                    }
                    if manager.is_cancelled(generation) {
                        manager.stop_watcher();
                        return;
                    }
                    manager.spawn_event_worker(
                        app.clone(),
                        generation,
                        config,
                        receiver,
                        Vec::new(),
                    );
                    manager.status.lock().unwrap().phase = "ready".into();
                    manager.emit_status(&app);
                });
                Ok(())
            }
        } else {
            self.start(app, true)
        }
    }

    pub fn pause<S: SearchStatusSink>(&self, app: &S) {
        self.cancel_and_wait();
        self.status.lock().unwrap().phase = "paused".into();
        self.emit_status(app);
    }

    fn cancel_and_wait(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.stop_watcher();
        let _operation = self
            .operation
            .lock()
            .unwrap_or_else(|error| error.into_inner());
    }

    pub fn clear<S: SearchStatusSink>(&self, app: &S) -> anyhow::Result<()> {
        self.pause(app);
        let _operation = self
            .operation
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        clear_rows(&self.db_path)?;
        self.clear_query_index()?;
        {
            let config = self.config.lock().unwrap();
            write_config(&self.config_path, &config)?;
            *self.status.lock().unwrap() = SearchStatus::disabled(&config);
        }
        self.emit_status(app);
        Ok(())
    }

    pub fn set_exclusions<S: SearchStatusSink>(
        self: &Arc<Self>,
        app: S,
        exclusions: Vec<String>,
    ) -> anyhow::Result<()> {
        let normalized = normalize_unique_paths(exclusions);
        {
            let mut config = self.config.lock().unwrap();
            config.exclusions = normalized;
            write_config(&self.config_path, &config)?;
        }
        self.start(app, true)
    }

    pub fn query(
        &self,
        query: &str,
        filter: &str,
        offset: u32,
        limit: u32,
    ) -> anyhow::Result<SearchPage> {
        let started = std::time::Instant::now();
        let terms = query
            .split_whitespace()
            .map(|term| term.to_lowercase())
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        if terms.is_empty() {
            return Ok(SearchPage {
                items: Vec::new(),
                total: 0,
                partial: self.status.lock().unwrap().phase != "ready",
                elapsed_ms: 0,
            });
        }
        let limit = limit.clamp(1, QUERY_LIMIT_MAX);
        let phase = self.status.lock().unwrap().phase.clone();
        let filter_sql = match filter {
            "log" => " AND f.is_log = 1 AND f.is_archive = 0",
            "archive" => " AND f.is_archive = 1",
            _ => "",
        };
        let (mut items, total) = match self.query_tantivy(&terms, filter, offset, limit)? {
            Some(result) => result,
            None => {
                let connection = open_database(&self.db_path)?;
                query_like(&connection, &terms, filter_sql, offset, limit)?
            }
        };
        enrich_visible_metadata(&mut items);
        Ok(SearchPage {
            items,
            total,
            partial: phase != "ready",
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    pub fn remove_stale_path(&self, path: &str) -> anyhow::Result<()> {
        let connection = open_database(&self.db_path)?;
        connection.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        self.drain_query_index_changes()?;
        Ok(())
    }

    fn index_files(&self, files: &[IndexedFile]) -> anyhow::Result<()> {
        let entries = files.iter().map(search_index_entry).collect::<Vec<_>>();
        let bulk = self.query_index_bulk.load(Ordering::Acquire);
        if bulk && self.query_index_staged.load(Ordering::Acquire) {
            if let Some(index) = self.staged_query_index.lock().unwrap().as_mut() {
                index.add_batch(&entries)?;
            }
        } else if let Some(index) = self.query_index.lock().unwrap().as_mut() {
            if bulk {
                index.add_batch(&entries)?;
            } else {
                index.upsert_batch(&entries)?;
            }
        }
        if bulk {
            let mut status = self.status.lock().unwrap();
            status.indexed_files = status.indexed_files.saturating_add(files.len() as u64);
        }
        Ok(())
    }

    fn begin_query_index_bulk(&self) -> anyhow::Result<()> {
        self.staged_query_index.lock().unwrap().take();
        let staging_path = query_index_staging_path(&self.query_index_path);
        if staging_path.exists() {
            retry_query_index_fs("prepare-remove-staging", &staging_path, None, || {
                fs::remove_dir_all(&staging_path)
            })?;
        }
        let has_complete_snapshot = self
            .query_index
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|index| index.num_docs() > 0);
        let needs_new_schema = self.query_index.lock().unwrap().is_none();
        if has_complete_snapshot || needs_new_schema {
            let mut staging = SearchIndex::open(&staging_path)?;
            staging.begin_bulk()?;
            *self.staged_query_index.lock().unwrap() = Some(staging);
            self.query_index_staged.store(true, Ordering::Release);
        } else if let Some(index) = self.query_index.lock().unwrap().as_mut() {
            index.begin_bulk()?;
            self.query_index_staged.store(false, Ordering::Release);
        }
        self.query_index_bulk.store(true, Ordering::Release);
        Ok(())
    }

    fn finish_query_index_bulk(&self) -> anyhow::Result<()> {
        if self.query_index_staged.load(Ordering::Acquire) {
            if let Some(mut staging) = self.staged_query_index.lock().unwrap().take() {
                staging.finish_bulk()?;
                if let Err(error) = self.validate_query_index_scopes(&staging) {
                    let _ = staging.close();
                    return Err(error);
                }
                staging.close()?;
            }
            self.activate_staged_query_index()?;
        } else if let Some(index) = self.query_index.lock().unwrap().as_mut() {
            index.finish_bulk()?;
        }
        self.query_index_bulk.store(false, Ordering::Release);
        self.query_index_staged.store(false, Ordering::Release);
        self.query_index_ready.store(true, Ordering::Release);
        Ok(())
    }

    fn validate_query_index_scopes(&self, index: &SearchIndex) -> anyhow::Result<()> {
        let connection = open_database(&self.db_path)?;
        let mut statement =
            connection.prepare("SELECT root, COUNT(*) FROM files GROUP BY root ORDER BY root")?;
        let expected = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (scope_key, expected_count) in expected {
            let actual_count = index.num_docs_for_scope(&scope_key)?;
            if actual_count != expected_count {
                anyhow::bail!(
                    "query-index scope count mismatch scope={} expected={} actual={}",
                    scope_key,
                    expected_count,
                    actual_count
                );
            }
        }
        Ok(())
    }

    fn clear_query_index(&self) -> anyhow::Result<()> {
        self.staged_query_index.lock().unwrap().take();
        self.query_index_staged.store(false, Ordering::Release);
        let staging_path = query_index_staging_path(&self.query_index_path);
        if staging_path.exists() {
            retry_query_index_fs("clear-remove-staging", &staging_path, None, || {
                fs::remove_dir_all(&staging_path)
            })?;
        }
        if let Some(index) = self.query_index.lock().unwrap().as_mut() {
            index.clear()?;
        }
        Ok(())
    }

    fn commit_query_index(&self) -> anyhow::Result<()> {
        if self.query_index_staged.load(Ordering::Acquire) {
            if let Some(index) = self.staged_query_index.lock().unwrap().as_mut() {
                index.commit()?;
            }
            return Ok(());
        }
        if let Some(index) = self.query_index.lock().unwrap().as_mut() {
            index.commit()?;
        }
        Ok(())
    }

    fn activate_staged_query_index(&self) -> anyhow::Result<()> {
        let staging = query_index_staging_path(&self.query_index_path);
        let previous = query_index_previous_path(&self.query_index_path);
        let active_index = self.query_index.lock().unwrap().take();
        if let Some(index) = active_index {
            if let Err(error) = index.close() {
                *self.query_index.lock().unwrap() = SearchIndex::open(&self.query_index_path).ok();
                return Err(anyhow::anyhow!(
                    "query-index stage=close-active-before-switch source={} target=<none>: {error}",
                    self.query_index_path.display()
                ));
            }
        }
        if previous.exists() {
            retry_query_index_fs("switch-remove-previous", &previous, None, || {
                fs::remove_dir_all(&previous)
            })?;
        }
        if self.query_index_path.exists() {
            retry_query_index_fs(
                "switch-active-to-previous",
                &self.query_index_path,
                Some(&previous),
                || fs::rename(&self.query_index_path, &previous),
            )?;
        }
        if let Err(error) = retry_query_index_fs(
            "switch-staging-to-active",
            &staging,
            Some(&self.query_index_path),
            || fs::rename(&staging, &self.query_index_path),
        ) {
            if previous.exists() && !self.query_index_path.exists() {
                let _ = retry_query_index_fs(
                    "rollback-previous-to-active",
                    &previous,
                    Some(&self.query_index_path),
                    || fs::rename(&previous, &self.query_index_path),
                );
            }
            *self.query_index.lock().unwrap() = SearchIndex::open(&self.query_index_path).ok();
            return Err(error);
        }
        match SearchIndex::open(&self.query_index_path) {
            Ok(index) => {
                *self.query_index.lock().unwrap() = Some(index);
                if previous.exists() {
                    retry_query_index_fs("switch-remove-old-active", &previous, None, || {
                        fs::remove_dir_all(&previous)
                    })?;
                }
                Ok(())
            }
            Err(error) => {
                let failed = query_index_staging_path(&self.query_index_path);
                let _ = retry_query_index_fs(
                    "rollback-failed-active-to-staging",
                    &self.query_index_path,
                    Some(&failed),
                    || fs::rename(&self.query_index_path, &failed),
                );
                if previous.exists() {
                    let _ = retry_query_index_fs(
                        "rollback-previous-to-active",
                        &previous,
                        Some(&self.query_index_path),
                        || fs::rename(&previous, &self.query_index_path),
                    );
                }
                *self.query_index.lock().unwrap() = SearchIndex::open(&self.query_index_path).ok();
                Err(error)
            }
        }
    }

    fn query_tantivy(
        &self,
        terms: &[String],
        filter: &str,
        offset: u32,
        limit: u32,
    ) -> anyhow::Result<Option<(Vec<SearchResultItem>, u64)>> {
        if !self.query_index_ready.load(Ordering::Acquire) {
            return Ok(None);
        }
        self.active_queries.fetch_add(1, Ordering::Relaxed);
        let _query_activity = QueryActivityGuard(&self.active_queries);
        let scanning = self.status.lock().unwrap().phase == "scanning";
        if scanning && self.query_index_staged.load(Ordering::Acquire) {
            let guard = self.staged_query_index.lock().unwrap();
            if let Some(index) = guard.as_ref().filter(|index| index.num_docs() > 0) {
                return search_query_index(index, terms, filter, offset, limit).map(Some);
            }
        }
        let guard = self.query_index.lock().unwrap();
        let Some(index) = guard.as_ref().filter(|index| index.num_docs() > 0) else {
            return Ok(None);
        };
        search_query_index(index, terms, filter, offset, limit).map(Some)
    }

    fn ensure_query_index_matches_database(&self) -> anyhow::Result<()> {
        if self.query_index_ready.load(Ordering::Acquire) {
            return Ok(());
        }
        self.rebuild_query_index_from_database()
    }

    fn rebuild_query_index_from_database(&self) -> anyhow::Result<()> {
        self.query_index_ready.store(false, Ordering::Release);
        if self.query_index.lock().unwrap().is_none() {
            self.begin_query_index_bulk()?;
            let result = {
                let mut guard = self.staged_query_index.lock().unwrap();
                let Some(index) = guard.as_mut() else {
                    anyhow::bail!("query-index migration did not create a staging index");
                };
                self.populate_query_index_from_database(index)
            };
            if let Err(error) = result {
                let _ = self.clear_query_index();
                return Err(error);
            }
            self.finish_query_index_bulk()?;
            return Ok(());
        }
        let mut guard = self.query_index.lock().unwrap();
        let Some(index) = guard.as_mut() else {
            return Ok(());
        };
        index.begin_bulk()?;
        self.populate_query_index_from_database(index)?;
        index.finish_bulk()?;
        self.query_index_ready.store(true, Ordering::Release);
        Ok(())
    }

    fn populate_query_index_from_database(&self, index: &mut SearchIndex) -> anyhow::Result<()> {
        let connection = open_database(&self.db_path)?;
        let mut statement = connection
            .prepare("SELECT path, name, root, is_log, is_archive FROM files ORDER BY rowid")?;
        let mut rows = statement.query([])?;
        let mut batch = Vec::with_capacity(SCAN_WRITE_BATCH);
        while let Some(row) = rows.next()? {
            batch.push(SearchIndexEntry {
                path: row.get(0)?,
                name: row.get(1)?,
                scope_key: row.get(2)?,
                is_log: row.get(3)?,
                is_archive: row.get(4)?,
            });
            if batch.len() == SCAN_WRITE_BATCH {
                index.add_batch(&batch)?;
                batch.clear();
            }
        }
        if !batch.is_empty() {
            index.add_batch(&batch)?;
        }
        Ok(())
    }

    fn drain_query_index_changes(&self) -> anyhow::Result<()> {
        if self.query_index.lock().unwrap().is_none() {
            return Ok(());
        }
        let connection = open_database(&self.db_path)?;
        loop {
            let changes = {
                let mut statement = connection.prepare(
                    "SELECT path, operation FROM search_index_changes ORDER BY rowid LIMIT 4096",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            if changes.is_empty() {
                break;
            }
            let mut upserts = Vec::new();
            let upsert_paths = changes
                .iter()
                .filter(|(_, operation)| *operation == 1)
                .map(|(path, _)| path)
                .collect::<Vec<_>>();
            for path_chunk in upsert_paths.chunks(500) {
                let placeholders = std::iter::repeat("?")
                    .take(path_chunk.len())
                    .collect::<Vec<_>>()
                    .join(",");
                let sql = format!(
                    "SELECT path, name, root, is_log, is_archive FROM files WHERE path IN ({placeholders})"
                );
                let mut load = connection.prepare(&sql)?;
                let rows = load.query_map(params_from_iter(path_chunk.iter()), |row| {
                    Ok(SearchIndexEntry {
                        path: row.get(0)?,
                        name: row.get(1)?,
                        scope_key: row.get(2)?,
                        is_log: row.get(3)?,
                        is_archive: row.get(4)?,
                    })
                })?;
                upserts.extend(rows.collect::<Result<Vec<_>, _>>()?);
            }
            let paths = changes
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            if let Some(index) = self.query_index.lock().unwrap().as_mut() {
                index.apply_changes(&paths, &upserts)?;
            }
            let tx = connection.unchecked_transaction()?;
            {
                let mut delete =
                    tx.prepare_cached("DELETE FROM search_index_changes WHERE path = ?1")?;
                for path in &paths {
                    delete.execute(params![path])?;
                }
            }
            tx.commit()?;
        }
        Ok(())
    }

    fn is_cancelled(&self, generation: u64) -> bool {
        self.cancel.load(Ordering::Relaxed) || self.generation.load(Ordering::Relaxed) != generation
    }

    fn finish_with_error<S: SearchStatusSink>(
        self: &Arc<Self>,
        app: &S,
        generation: u64,
        error: anyhow::Error,
    ) {
        if self.is_cancelled(generation) {
            return;
        }
        if is_database_corruption(&error)
            && self
                .corruption_recovery
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            let recovery = (|| -> anyhow::Result<()> {
                self.stop_watcher();
                quarantine_database(&self.db_path)?;
                // A quarantined database leaves no schema behind. The rebuild
                // path assumes metadata/files already exist, so recreate the
                // empty persistent store before scheduling the scan.
                initialize_database_with_query(&self.db_path, None)?;
                self.query_index_ready.store(false, Ordering::Release);
                if self.clear_query_index().is_err() {
                    if self.query_index_path.exists() {
                        fs::remove_dir_all(&self.query_index_path)?;
                    }
                    *self.query_index.lock().unwrap() = None;
                }
                self.start(app.clone(), true)?;
                Ok(())
            })();
            if recovery.is_ok() {
                return;
            }
        }
        let diagnostic = self.operation_diagnostic_context();
        let error_message = format!("{error}; {diagnostic}");
        let mut status = self.status.lock().unwrap();
        status.phase = "error".into();
        status.error = Some(error_message.clone());
        drop(status);
        if let Some(snapshot) = self.operation_snapshot.lock().unwrap().as_mut() {
            snapshot.final_phase = "error".into();
            snapshot.converged_ms = Some(system_time_ms(SystemTime::now()).unwrap_or(0));
            snapshot.error = Some(error_message);
        }
        self.emit_status(app);
    }

    fn operation_diagnostic_context(&self) -> String {
        let operation_id = self
            .operation_snapshot
            .lock()
            .unwrap()
            .as_ref()
            .map(|snapshot| snapshot.operation_id.clone())
            .unwrap_or_else(|| "<none>".into());
        let active = self.query_index_path.exists();
        let staging = query_index_staging_path(&self.query_index_path).exists();
        let previous = query_index_previous_path(&self.query_index_path).exists();
        format!(
            "operation_id={operation_id} active={active} staging={staging} previous={previous} concurrent_queries={}",
            self.active_queries.load(Ordering::Relaxed)
        )
    }

    fn emit_status<S: SearchStatusSink>(&self, app: &S) {
        let snapshot = {
            let mut status = self.status.lock().unwrap();
            refresh_provider_elapsed(&mut status);
            status.clone()
        };
        app.emit_search_status(snapshot);
    }

    fn sync_operation_scopes(&self) {
        let providers = self.status.lock().unwrap().providers.clone();
        let mut operation = self.operation_snapshot.lock().unwrap();
        let Some(snapshot) = operation.as_mut() else {
            return;
        };
        for provider in providers {
            if let Some(scope) = snapshot
                .scopes
                .iter_mut()
                .find(|scope| scope.scope_key == provider.root)
            {
                scope.provider = provider.provider;
                scope.phase = provider.phase;
                scope.discovered_records = provider.discovered_records;
                scope.searchable_files = provider.searchable_files;
                scope.elapsed_ms = provider.elapsed_ms;
                scope.error = provider.fallback_reason;
            }
        }
    }

    #[cfg(all(test, windows))]
    fn operation_snapshot_for_report(&self) -> Option<IndexOperationSnapshot> {
        self.operation_snapshot.lock().unwrap().clone()
    }

    fn refresh_counts(&self) {
        let Ok(connection) = open_database(&self.db_path) else {
            return;
        };
        let root_counts = if self.query_index_bulk.load(Ordering::Acquire)
            || self.persistence_recovery.load(Ordering::Acquire)
        {
            None
        } else {
            let roots = self
                .status
                .lock()
                .unwrap()
                .providers
                .iter()
                .map(|item| item.root.clone())
                .collect::<Vec<_>>();
            Some(
                roots
                    .into_iter()
                    .map(|root| {
                        let count = connection
                            .query_row(
                                "SELECT COUNT(*) FROM files WHERE root = ?1",
                                params![&root],
                                |row| row.get::<_, u64>(0),
                            )
                            .unwrap_or(0);
                        (root, count)
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let index_bytes = database_size(&self.db_path);
        let database_indexed_files = root_counts
            .as_ref()
            .map(|counts| counts.iter().map(|(_, count)| *count).sum::<u64>());
        let query_indexed_files = self
            .query_index
            .lock()
            .unwrap()
            .as_ref()
            .map(SearchIndex::num_docs);
        let mut status = self.status.lock().unwrap();
        if status.phase == "ready"
            && database_indexed_files.is_some()
            && database_indexed_files != query_indexed_files
        {
            eprintln!(
                "[search-index] count-mismatch database={} query={}",
                database_indexed_files.unwrap_or(0),
                query_indexed_files.unwrap_or(0)
            );
        }
        if let Some(root_counts) = root_counts {
            for provider in &mut status.providers {
                provider.searchable_files = root_counts
                    .iter()
                    .find(|(root, _)| root == &provider.root)
                    .map(|(_, count)| *count)
                    .unwrap_or(0);
            }
        }
        status.index_bytes = index_bytes;
        refresh_provider_totals(&mut status);
    }

    fn stop_watcher(&self) {
        self.event_sender.lock().unwrap().take();
        self.watcher.lock().unwrap().take();
    }

    fn install_watcher(self: &Arc<Self>, config: &SearchConfig) -> anyhow::Result<Receiver<Event>> {
        self.stop_watcher();
        self.event_dirty.store(false, Ordering::SeqCst);
        let (sender, receiver) = sync_channel::<Event>(EVENT_QUEUE_CAPACITY);
        let callback_sender = sender.clone();
        let dirty = Arc::clone(&self.event_dirty);
        let event_roots = config.roots.clone();
        let event_exclusions = config.exclusions.clone();
        let event_internal_root = self.db_path.parent().map(Path::to_path_buf);
        let mut watcher = RecommendedWatcher::new(
            move |result: notify::Result<Event>| {
                let Ok(mut event) = result else {
                    dirty.store(true, Ordering::Relaxed);
                    return;
                };
                let event_kind = event.kind;
                event.paths.retain(|path| {
                    containing_root(path, &event_roots).is_some()
                        && !is_excluded(path, &event_exclusions)
                        && !event_internal_root
                            .as_deref()
                            .is_some_and(|internal| path_is_within(path, internal))
                        && !is_platform_skipped_directory(path)
                        && event_path_requires_reconcile(&event_kind, path)
                });
                if event.paths.is_empty() {
                    return;
                }
                enqueue_event(&callback_sender, &dirty, event);
            },
            NotifyConfig::default(),
        )?;
        for root in &config.roots {
            let path = Path::new(root);
            if path.is_dir() {
                watcher.watch(path, RecursiveMode::Recursive)?;
            }
        }
        *self.watcher.lock().unwrap() = Some(watcher);
        *self.event_sender.lock().unwrap() = Some(sender);

        Ok(receiver)
    }

    fn spawn_event_worker<S: SearchStatusSink>(
        self: &Arc<Self>,
        app: S,
        generation: u64,
        config: SearchConfig,
        receiver: Receiver<Event>,
        mut pending_paths: Vec<PathBuf>,
    ) {
        let manager = Arc::clone(self);
        std::thread::spawn(move || loop {
            if manager.is_cancelled(generation) {
                break;
            }
            let mut paths = if pending_paths.is_empty() {
                let first = match receiver.recv_timeout(Duration::from_millis(500)) {
                    Ok(event) => event,
                    Err(RecvTimeoutError::Timeout) => {
                        if manager.event_sender.lock().unwrap().is_none() {
                            break;
                        }
                        if manager.event_dirty.swap(false, Ordering::SeqCst) {
                            let _ = manager.start(app.clone(), true);
                            break;
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                first.paths
            } else {
                std::mem::take(&mut pending_paths)
            };
            while let Ok(event) = receiver.try_recv() {
                paths.extend(event.paths);
                if paths.len() >= EVENT_BATCH {
                    break;
                }
            }
            paths.sort();
            paths.dedup();
            let _operation = manager
                .operation
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if manager.is_cancelled(generation) {
                break;
            }
            if let Err(error) = apply_event_paths(&manager.db_path, &config, &paths)
                .and_then(|_| manager.drain_query_index_changes())
            {
                manager.status.lock().unwrap().error = Some(error.to_string());
                manager.emit_status(&app);
            } else {
                manager.refresh_counts();
                manager.emit_status(&app);
            }
        });
    }
}

fn is_database_corruption(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("database disk image is malformed")
        || message.contains("database corruption")
        || message.contains("malformed database")
        || message.contains("no such table: metadata")
}

fn quarantine_database(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let suffix = system_time_ms(SystemTime::now()).unwrap_or(0);
    let base = path.with_extension(format!("sqlite3.corrupt-{suffix}"));
    fs::rename(path, &base)?;
    for extension in ["sqlite3-wal", "sqlite3-shm"] {
        let sidecar = path.with_file_name(format!("file-search.{extension}"));
        if sidecar.exists() {
            let target = sidecar.with_file_name(format!(
                "{}.corrupt-{suffix}",
                sidecar.file_name().unwrap().to_string_lossy()
            ));
            let _ = fs::rename(sidecar, target);
        }
    }
    Ok(())
}

fn search_query_index(
    index: &SearchIndex,
    terms: &[String],
    filter: &str,
    offset: u32,
    limit: u32,
) -> anyhow::Result<(Vec<SearchResultItem>, u64)> {
    let (entries, total) = index.search(terms, filter, offset, limit)?;
    let items = entries
        .into_iter()
        .map(|entry| {
            let content_type = content_type_for_name(&entry.name).into();
            SearchResultItem {
                parent: Path::new(&entry.path)
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: entry.path,
                name: entry.name,
                kind: if entry.is_archive {
                    "archive".into()
                } else if entry.is_log {
                    "log".into()
                } else {
                    "file".into()
                },
                size: 0,
                modified_ms: None,
                readable: false,
                content_type,
                is_log: entry.is_log,
                is_archive: entry.is_archive,
            }
        })
        .collect();
    Ok((items, total))
}

fn event_path_requires_reconcile(kind: &notify::EventKind, path: &Path) -> bool {
    use notify::event::ModifyKind;
    use notify::EventKind;

    match kind {
        EventKind::Access(_) => false,
        EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)) => !path.is_dir(),
        _ => true,
    }
}

fn search_index_entry(file: &IndexedFile) -> SearchIndexEntry {
    SearchIndexEntry {
        path: file.path.clone(),
        name: file.name.clone(),
        scope_key: file.root.clone(),
        is_log: file.is_log,
        is_archive: file.is_archive,
    }
}

fn enqueue_event(sender: &SyncSender<Event>, dirty: &AtomicBool, event: Event) {
    if let Err(TrySendError::Full(_)) = sender.try_send(event) {
        dirty.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct ScanOutcome {
    #[cfg(windows)]
    ntfs_finalize_jobs: Vec<NtfsFinalizeJob>,
}

impl ScanOutcome {
    fn has_deferred_persistence(&self) -> bool {
        #[cfg(windows)]
        {
            !self.ntfs_finalize_jobs.is_empty()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

#[cfg(windows)]
fn compatible_provider_fallback_reason(
    root: &str,
    failure: &anyhow::Error,
) -> anyhow::Result<Option<String>> {
    if !failure
        .chain()
        .any(|cause| cause.downcast_ref::<ServiceFailure>().is_some())
    {
        return Ok(None);
    }
    let root_path = Path::new(root);
    let metadata = fs::metadata(root_path)
        .map_err(|error| anyhow::anyhow!("兼容 provider 无法读取卷根 {root}: {error}"))?;
    if !metadata.is_dir() {
        anyhow::bail!("兼容 provider 的卷根不是目录: {root}");
    }
    fs::read_dir(root_path)
        .map_err(|error| anyhow::anyhow!("兼容 provider 无权枚举卷根 {root}: {error}"))?;
    Ok(Some(format!(
        "Windows NTFS 快速索引服务不可用，已自动降级到兼容目录扫描: {failure}"
    )))
}

#[cfg(windows)]
fn clear_root_for_compatible_provider(db_path: &Path, root: &str) -> anyhow::Result<()> {
    let mut connection = open_database(db_path)?;
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM files WHERE root = ?1", params![root])?;
    transaction.execute("DELETE FROM ntfs_nodes WHERE root = ?1", params![root])?;
    transaction.execute("DELETE FROM search_volumes WHERE root = ?1", params![root])?;
    transaction.commit()?;
    Ok(())
}

fn scan_with_providers<S: SearchStatusSink>(
    manager: &Arc<FileSearchManager>,
    app: &S,
    generation: u64,
    config: &SearchConfig,
) -> anyhow::Result<ScanOutcome> {
    let scan_started = std::time::Instant::now();
    #[cfg(windows)]
    {
        let mut folder_roots = Vec::new();
        let mut ntfs_roots = Vec::new();
        let mut finalize_jobs = Vec::new();
        let mut fallback_roots = Vec::new();
        for root in &config.roots {
            if manager.is_cancelled(generation) {
                return Ok(ScanOutcome::default());
            }
            let Some(volume) = ntfs_volume_letter(root) else {
                folder_roots.push(root.clone());
                continue;
            };
            ntfs_roots.push((root.clone(), volume));
        }
        let work = |root: String, volume| {
            set_provider_stage(
                manager,
                app,
                &root,
                "windowsNtfs",
                "scanning",
                "connecting",
                None,
            );
            enumerate_ntfs_volume_snapshot(manager, app, generation, config, &root, volume)
        };
        let completed = run_ntfs_volume_tasks(
            ntfs_roots,
            &work,
            &|| manager.is_cancelled(generation),
            |root, snapshot| {
                match snapshot.and_then(|snapshot| {
                    set_provider_stage(
                        manager,
                        app,
                        &root,
                        "windowsNtfs",
                        "merging",
                        "resolvingPaths",
                        None,
                    );
                    index_ntfs_volume_paths(manager, app, generation, snapshot)
                }) {
                    Ok(job) => {
                        set_provider_stage(
                            manager,
                            app,
                            &root,
                            "windowsNtfs",
                            "merging",
                            "persisting",
                            None,
                        );
                        finalize_jobs.push(job);
                    }
                    Err(error) if !manager.is_cancelled(generation) => {
                        if let Some(reason) = compatible_provider_fallback_reason(&root, &error)? {
                            eprintln!(
                                "[search-index] root={root} strategy=folder-fallback reason={error}"
                            );
                            set_provider_stage(
                                manager,
                                app,
                                &root,
                                "folderScan",
                                "fallback",
                                "fallback",
                                Some(reason),
                            );
                            fallback_roots.push(root);
                            return Ok(());
                        }
                        set_provider_status(
                            manager,
                            app,
                            &root,
                            "windowsNtfs",
                            "error",
                            Some(error.to_string()),
                        );
                        return Err(error);
                    }
                    Err(_) => {}
                }
                Ok(())
            },
        )?;
        if !completed {
            return Ok(ScanOutcome::default());
        }
        folder_roots.extend(fallback_roots);
        scan_folder_roots(manager, app, generation, config, &folder_roots)?;
        eprintln!(
            "[search-index] generation={generation} all-volumes-query-ready elapsed_ms={}",
            scan_started.elapsed().as_millis()
        );
        Ok(ScanOutcome {
            ntfs_finalize_jobs: finalize_jobs,
        })
    }
    #[cfg(not(windows))]
    {
        for root in &config.roots {
            set_provider_status(manager, app, root, "folderScan", "scanning", None);
        }
        scan_folder_roots(manager, app, generation, config, &config.roots)?;
        manager.status.lock().unwrap().phase = "finalizing".into();
        manager.refresh_counts();
        manager.emit_status(app);
        eprintln!(
            "[search-index] generation={generation} folder-scan-complete elapsed_ms={}",
            scan_started.elapsed().as_millis()
        );
        Ok(ScanOutcome::default())
    }
}

fn set_provider_status<S: SearchStatusSink>(
    manager: &FileSearchManager,
    app: &S,
    root: &str,
    provider: &str,
    phase: &str,
    fallback_reason: Option<String>,
) {
    set_provider_stage(manager, app, root, provider, phase, phase, fallback_reason);
}

fn set_provider_stage<S: SearchStatusSink>(
    manager: &FileSearchManager,
    app: &S,
    root: &str,
    provider: &str,
    phase: &str,
    stage: &str,
    fallback_reason: Option<String>,
) {
    let now = system_time_ms(SystemTime::now()).unwrap_or(0);
    let mut status = manager.status.lock().unwrap();
    update_provider_stage_at(
        &mut status,
        root,
        provider,
        phase,
        stage,
        fallback_reason,
        now,
    );
    drop(status);
    manager.sync_operation_scopes();
    manager.emit_status(app);
}

fn add_provider_progress<S: SearchStatusSink>(
    manager: &FileSearchManager,
    app: &S,
    root: &str,
    stage: &str,
    discovered: u64,
    searchable: u64,
) {
    let now = system_time_ms(SystemTime::now()).unwrap_or(0);
    let mut status = manager.status.lock().unwrap();
    let mut first_searchable = None;
    if let Some(item) = status.providers.iter_mut().find(|item| item.root == root) {
        if item.stage != stage {
            item.stage = stage.into();
            item.stage_started_ms = Some(now);
            item.stage_elapsed_ms = 0;
        }
        if item.started_ms.is_none() {
            item.started_ms = Some(now);
        }
        item.discovered_records = item.discovered_records.saturating_add(discovered);
        let is_first_searchable = searchable > 0 && item.searchable_files == 0;
        item.searchable_files = item.searchable_files.saturating_add(searchable);
        item.elapsed_ms = item
            .started_ms
            .map(|started| now.saturating_sub(started))
            .unwrap_or(0);
        item.stage_elapsed_ms = item
            .stage_started_ms
            .map(|started| now.saturating_sub(started))
            .unwrap_or(0);
        if is_first_searchable {
            first_searchable = Some(item.elapsed_ms);
        }
    }
    refresh_provider_totals(&mut status);
    drop(status);
    manager.sync_operation_scopes();
    if let Some(elapsed_ms) = first_searchable {
        eprintln!("[search-index] root={root} first-searchable-batch elapsed_ms={elapsed_ms}");
    }
    manager.emit_status(app);
}

#[cfg(windows)]
fn set_provider_searchable_files<S: SearchStatusSink>(
    manager: &FileSearchManager,
    app: &S,
    root: &str,
    searchable: u64,
) {
    let mut status = manager.status.lock().unwrap();
    if let Some(item) = status.providers.iter_mut().find(|item| item.root == root) {
        item.searchable_files = searchable;
    }
    refresh_provider_totals(&mut status);
    drop(status);
    manager.emit_status(app);
}

#[cfg(windows)]
fn indexed_root_count(db_path: &Path, root: &str) -> u64 {
    open_database(db_path)
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM files WHERE root = ?1",
                    params![root],
                    |row| row.get::<_, u64>(0),
                )
                .map_err(Into::into)
        })
        .unwrap_or(0)
}

fn refresh_provider_elapsed(status: &mut SearchStatus) {
    let now = system_time_ms(SystemTime::now()).unwrap_or(0);
    for item in &mut status.providers {
        if let Some(started) = item.started_ms {
            if item.completed_ms.is_none() {
                item.elapsed_ms = now.saturating_sub(started);
            }
        }
        if let Some(stage_started) = item.stage_started_ms {
            if item.completed_ms.is_none() {
                item.stage_elapsed_ms = now.saturating_sub(stage_started);
            }
        }
    }
    refresh_provider_totals(status);
}

fn update_provider_stage_at(
    status: &mut SearchStatus,
    root: &str,
    provider: &str,
    phase: &str,
    stage: &str,
    fallback_reason: Option<String>,
    now: u64,
) {
    if let Some(item) = status.providers.iter_mut().find(|item| item.root == root) {
        let stage_changed = item.stage != stage;
        item.provider = provider.into();
        item.phase = phase.into();
        if phase != "pending" && item.started_ms.is_none() {
            item.started_ms = Some(now);
        }
        if stage_changed || item.stage_started_ms.is_none() {
            item.stage = stage.into();
            item.stage_started_ms = Some(now);
            item.stage_elapsed_ms = 0;
        }
        item.elapsed_ms = item
            .started_ms
            .map(|started| now.saturating_sub(started))
            .unwrap_or(0);
        item.stage_elapsed_ms = item
            .stage_started_ms
            .map(|started| now.saturating_sub(started))
            .unwrap_or(0);
        item.completed_ms = (provider_phase_is_terminal(phase)
            && matches!(stage, "ready" | "fallback" | "error"))
        .then_some(now);
        item.fallback_reason = fallback_reason;
    }
    refresh_provider_totals(status);
}

fn provider_phase_is_terminal(phase: &str) -> bool {
    matches!(phase, "ready" | "fallback" | "error")
}

fn refresh_provider_totals(status: &mut SearchStatus) {
    status.scanned_files = status
        .providers
        .iter()
        .map(|item| item.discovered_records)
        .sum();
    status.indexed_files = status
        .providers
        .iter()
        .map(|item| item.searchable_files)
        .sum();
}

#[cfg(windows)]
struct NtfsFinalizeJob {
    root: String,
    volume: char,
    exclusions: Vec<String>,
    journal_before: UsnJournalInfo,
    records: Vec<MftRecord>,
}

#[cfg(windows)]
fn enumerate_ntfs_volume_snapshot<S: SearchStatusSink>(
    manager: &Arc<FileSearchManager>,
    app: &S,
    generation: u64,
    config: &SearchConfig,
    root: &str,
    volume: char,
) -> anyhow::Result<NtfsFinalizeJob> {
    let volume_started = std::time::Instant::now();
    eprintln!("[search-index] root={root} stage=enumeratingMft event=start records=0");
    set_provider_stage(
        manager,
        app,
        root,
        "windowsNtfs",
        "scanning",
        "readingUsn",
        None,
    );
    let journal_before = query_usn_via_service(volume)?;
    let mut records = Vec::<MftRecord>::new();
    set_provider_stage(
        manager,
        app,
        root,
        "windowsNtfs",
        "scanning",
        "enumeratingMft",
        None,
    );
    let enumeration = enumerate_mft_via_service(volume, |batch| {
        if manager.is_cancelled(generation) {
            anyhow::bail!("MFT 枚举已取消");
        }
        add_provider_progress(manager, app, root, "enumeratingMft", batch.len() as u64, 0);
        records.extend(batch);
        Ok(())
    })?;
    eprintln!(
        "[search-index] root={root} volume={} stage=enumeratingMft event=complete records={} batches={} elapsed_ms={}",
        volume.to_ascii_uppercase(),
        enumeration.records,
        enumeration.batches,
        volume_started.elapsed().as_millis()
    );
    if manager.is_cancelled(generation) {
        anyhow::bail!("MFT enumeration was cancelled");
    }
    Ok(NtfsFinalizeJob {
        root: root.into(),
        volume,
        exclusions: config.exclusions.clone(),
        journal_before,
        records,
    })
}

#[cfg(windows)]
fn index_ntfs_volume_paths<S: SearchStatusSink>(
    manager: &Arc<FileSearchManager>,
    app: &S,
    generation: u64,
    snapshot: NtfsFinalizeJob,
) -> anyhow::Result<NtfsFinalizeJob> {
    let started = std::time::Instant::now();
    let root = snapshot.root.clone();
    let mut merge_elapsed = Duration::ZERO;
    let mut searchable_files = 0_u64;
    eprintln!("[search-index] root={root} stage=resolvingPaths event=start records=0");
    eprintln!("[search-index] root={root} stage=merging event=start records=0");
    let (_, records) = resolve_mft_files_in_batches_retain(
        &root,
        snapshot.records,
        NTFS_RESOLVE_BATCH,
        |entries| {
            if manager.is_cancelled(generation) {
                anyhow::bail!("MFT 路径重建已取消");
            }
            let files = entries
                .into_iter()
                .filter(|entry| !is_excluded(Path::new(&entry.path), &snapshot.exclusions))
                .map(|entry| indexed_mft_entry(&root, entry))
                .collect::<Vec<_>>();
            let merge_started = std::time::Instant::now();
            manager.index_files(&files)?;
            merge_elapsed = merge_elapsed.saturating_add(merge_started.elapsed());
            searchable_files = searchable_files.saturating_add(files.len() as u64);
            add_provider_progress(manager, app, &root, "resolvingPaths", 0, files.len() as u64);
            Ok(())
        },
    )?;
    let resolving_elapsed = started.elapsed().saturating_sub(merge_elapsed);
    eprintln!(
        "[search-index] root={root} stage=resolvingPaths event=complete records={searchable_files} elapsed_ms={}",
        resolving_elapsed.as_millis()
    );
    manager.commit_query_index()?;
    eprintln!(
        "[search-index] root={root} stage=merging event=complete records={searchable_files} elapsed_ms={}",
        merge_elapsed.as_millis()
    );
    Ok(NtfsFinalizeJob {
        root,
        volume: snapshot.volume,
        exclusions: snapshot.exclusions,
        journal_before: snapshot.journal_before,
        records,
    })
}

#[cfg(windows)]
fn ntfs_volume_worker_count(task_count: usize) -> usize {
    task_count.min(NTFS_VOLUME_WORKERS_MAX)
}

#[cfg(windows)]
fn run_ntfs_volume_tasks<T, Work, Consume, Cancel>(
    tasks: Vec<(String, char)>,
    work: &Work,
    is_cancelled: &Cancel,
    mut consume: Consume,
) -> anyhow::Result<bool>
where
    T: Send,
    Work: Fn(String, char) -> anyhow::Result<T> + Sync,
    Consume: FnMut(String, anyhow::Result<T>) -> anyhow::Result<()>,
    Cancel: Fn() -> bool + Sync,
{
    let task_count = tasks.len();
    if task_count == 0 {
        return Ok(true);
    }
    let tasks = Arc::new(Mutex::new(VecDeque::from(tasks)));
    let worker_count = ntfs_volume_worker_count(task_count);
    let (sender, receiver) = sync_channel(worker_count);
    std::thread::scope(|scope| -> anyhow::Result<bool> {
        for _ in 0..worker_count {
            let tasks = Arc::clone(&tasks);
            let sender = sender.clone();
            scope.spawn(move || loop {
                if is_cancelled() {
                    break;
                }
                let task = tasks.lock().unwrap().pop_front();
                let Some((root, volume)) = task else {
                    break;
                };
                let result = work(root.clone(), volume);
                if sender.send((root, result)).is_err() {
                    break;
                }
            });
        }
        drop(sender);

        for _ in 0..task_count {
            if is_cancelled() {
                return Ok(false);
            }
            let (root, result) = receiver.recv().map_err(|_| {
                anyhow::anyhow!("NTFS volume worker stopped before reporting its result")
            })?;
            consume(root, result)?;
        }
        Ok(true)
    })
}

#[cfg(windows)]
fn persist_and_finalize_ntfs_volume(
    db_path: &Path,
    job: NtfsFinalizeJob,
    rebuild_node_index: bool,
) -> anyhow::Result<()> {
    let NtfsFinalizeJob {
        root,
        volume,
        exclusions,
        journal_before,
        records: snapshot_records,
    } = job;
    let started = std::time::Instant::now();
    eprintln!(
        "[search-index] root={} stage=persisting event=start records={}",
        root,
        snapshot_records.len()
    );
    let directory_ids = snapshot_records
        .iter()
        .filter(|record| record.is_directory())
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    std::thread::scope(|scope| -> anyhow::Result<()> {
        let usn_reconciliation = scope.spawn(|| -> anyhow::Result<_> {
            let journal_after = query_usn_via_service(volume)?;
            if journal_after.journal_id != journal_before.journal_id
                || journal_before.next_usn < journal_after.first_usn
            {
                anyhow::bail!("USN Journal 在 MFT 快照期间失效，需要重新枚举该卷");
            }
            if persistence_usn_range_exceeds_limit(journal_before.next_usn, journal_after.next_usn)
            {
                return Ok((journal_after, Vec::new(), Some("bounded-usn-range-limit")));
            }
            let (changes, watcher_reconcile_reason) = collect_persistence_usn_changes(
                &directory_ids,
                MAX_USN_REPLAY_RECORDS,
                PERSISTENCE_USN_REPLAY_MAX_DURATION,
                |on_batch| {
                    read_usn_via_service(
                        volume,
                        journal_before.next_usn,
                        journal_before.journal_id,
                        journal_after.next_usn,
                        on_batch,
                    )
                },
            )?;
            Ok((journal_after, changes, watcher_reconcile_reason))
        });

        let mut connection = open_database(db_path)?;
        connection.pragma_update(None, "synchronous", "OFF")?;
        connection.pragma_update(None, "cache_size", -524_288)?;
        connection.pragma_update(None, "mmap_size", 268_435_456_i64)?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM files WHERE root = ?1", params![&root])?;
        let paths_started = std::time::Instant::now();
        let (_, records) = resolve_mft_files_in_batches_retain(
            &root,
            snapshot_records,
            NTFS_RESOLVE_BATCH,
            |entries| {
                let files = entries
                    .into_iter()
                    .filter(|entry| !is_excluded(Path::new(&entry.path), &exclusions))
                    .map(|entry| indexed_mft_entry(&root, entry))
                    .collect::<Vec<_>>();
                write_file_rows(&transaction, &files, false)
            },
        )?;
        transaction.commit()?;
        eprintln!(
            "[search-index] root={} sqlite-paths-ready records={} elapsed_ms={}",
            root,
            records.len(),
            paths_started.elapsed().as_millis()
        );
        let nodes_started = std::time::Instant::now();
        if rebuild_node_index {
            replace_ntfs_nodes(&mut connection, &root, &records)?;
        } else {
            replace_ntfs_node_rows(&mut connection, &root, &records)?;
        }
        eprintln!(
            "[search-index] root={} sqlite-nodes-ready records={} elapsed_ms={}",
            root,
            records.len(),
            nodes_started.elapsed().as_millis()
        );
        let (journal_after, changes, watcher_reconcile_reason) = usn_reconciliation
            .join()
            .map_err(|_| anyhow::anyhow!("USN reconciliation worker panicked"))??;
        if let Some(reason) = watcher_reconcile_reason {
            save_ntfs_volume_state(&connection, &root, volume, &journal_after, false)?;
            eprintln!(
                "[search-index] root={} strategy=watcher-reconcile snapshot_complete=false reason={} elapsed_ms={}",
                root,
                reason,
                started.elapsed().as_millis()
            );
            eprintln!(
                "[search-index] root={} stage=persisting event=complete records={} strategy=watcher-reconcile elapsed_ms={}",
                root,
                records.len(),
                started.elapsed().as_millis()
            );
            return Ok(());
        }
        apply_usn_changes(&mut connection, &root, &exclusions, changes)?;
        save_ntfs_volume_state(&connection, &root, volume, &journal_after, true)?;
        eprintln!(
            "[search-index] root={} stage=persisting event=complete records={} elapsed_ms={}",
            root,
            records.len(),
            started.elapsed().as_millis()
        );
        Ok(())
    })
}

#[cfg(windows)]
fn indexed_mft_entry(root: &str, entry: crate::ntfs::ResolvedMftEntry) -> IndexedFile {
    IndexedFile {
        is_log: is_log_name(&entry.name),
        is_archive: is_archive_name(&entry.name),
        path: entry.path,
        name: entry.name,
        root: root.into(),
        size: 0,
        modified_ms: None,
        file_id: Some(entry.id.as_bytes()),
        parent_id: Some(entry.parent_id.as_bytes()),
    }
}

#[cfg(windows)]
fn replace_ntfs_nodes(
    connection: &mut Connection,
    root: &str,
    records: &[MftRecord],
) -> anyhow::Result<()> {
    connection.execute_batch("DROP INDEX IF EXISTS ntfs_nodes_parent_idx")?;
    replace_ntfs_node_rows(connection, root, records)?;
    connection.execute_batch(
        "CREATE INDEX IF NOT EXISTS ntfs_nodes_parent_idx
           ON ntfs_nodes(root, parent_id)",
    )?;
    Ok(())
}

#[cfg(windows)]
fn replace_ntfs_node_rows(
    connection: &mut Connection,
    root: &str,
    records: &[MftRecord],
) -> anyhow::Result<()> {
    let tx = connection.transaction()?;
    tx.execute("DELETE FROM ntfs_nodes WHERE root = ?1", params![root])?;
    for chunk in records.chunks(5_000) {
        let placeholders = (0..chunk.len())
            .map(|_| "(?, ?, ?, ?, ?, ?)")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "INSERT INTO ntfs_nodes(root, file_id, parent_id, name, attributes, usn)
             VALUES {placeholders}"
        );
        let mut values = Vec::<rusqlite::types::Value>::with_capacity(chunk.len() * 6);
        for record in chunk {
            values.push(root.to_owned().into());
            values.push(record.id.as_bytes().to_vec().into());
            values.push(record.parent_id.as_bytes().to_vec().into());
            values.push(record.name.clone().into());
            values.push(i64::from(record.attributes).into());
            values.push(record.usn.into());
        }
        tx.execute(&sql, rusqlite::params_from_iter(values))?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(windows)]
fn begin_ntfs_nodes_bulk(db_path: &Path) -> anyhow::Result<()> {
    open_database(db_path)?.execute_batch("DROP INDEX IF EXISTS ntfs_nodes_parent_idx")?;
    Ok(())
}

#[cfg(windows)]
fn finish_ntfs_nodes_bulk(db_path: &Path) -> anyhow::Result<()> {
    open_database(db_path)?.execute_batch(
        "CREATE INDEX IF NOT EXISTS ntfs_nodes_parent_idx
           ON ntfs_nodes(root, parent_id)",
    )?;
    Ok(())
}

#[cfg(windows)]
fn load_ntfs_nodes(connection: &Connection, root: &str) -> anyhow::Result<Vec<MftRecord>> {
    load_ntfs_records(connection, root, false)
}

#[cfg(windows)]
fn load_ntfs_records(
    connection: &Connection,
    root: &str,
    directories_only: bool,
) -> anyhow::Result<Vec<MftRecord>> {
    let sql = if directories_only {
        "SELECT file_id, parent_id, name, attributes, usn
         FROM ntfs_nodes WHERE root = ?1 AND (attributes & 16) != 0"
    } else {
        "SELECT file_id, parent_id, name, attributes, usn
         FROM ntfs_nodes WHERE root = ?1"
    };
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params![root], |row| {
        let id = row.get::<_, Vec<u8>>(0)?;
        let parent_id = row.get::<_, Vec<u8>>(1)?;
        let to_id = |bytes: Vec<u8>| -> rusqlite::Result<crate::ntfs::FileId> {
            let bytes: [u8; 16] = bytes.try_into().map_err(|_| {
                rusqlite::Error::InvalidColumnType(0, "file_id".into(), rusqlite::types::Type::Blob)
            })?;
            Ok(crate::ntfs::FileId::from_bytes(bytes))
        };
        Ok(MftRecord {
            id: to_id(id)?,
            parent_id: to_id(parent_id)?,
            name: row.get(2)?,
            attributes: row.get(3)?,
            reason: 0,
            usn: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(windows)]
fn apply_usn_changes(
    connection: &mut Connection,
    root: &str,
    exclusions: &[String],
    changes: Vec<MftRecord>,
) -> anyhow::Result<()> {
    const USN_REASON_FILE_DELETE: u32 = 0x0000_0200;
    const USN_REASON_RENAME_OLD_NAME: u32 = 0x0000_1000;
    if changes.is_empty() {
        return Ok(());
    }
    let has_directory_change = {
        let mut attributes = connection
            .prepare_cached("SELECT attributes FROM ntfs_nodes WHERE root = ?1 AND file_id = ?2")?;
        changes.iter().try_fold(false, |found, change| {
            if found || change.is_directory() {
                return Ok(true);
            }
            let previous = attributes
                .query_row(params![root, change.id.as_bytes().as_slice()], |row| {
                    row.get::<_, u32>(0)
                })
                .optional()?;
            Ok::<_, anyhow::Error>(
                previous.is_some_and(|value| value & FILE_ATTRIBUTE_DIRECTORY != 0),
            )
        })?
    };
    if !has_directory_change {
        return apply_file_usn_changes(connection, root, exclusions, changes);
    }
    let old_records = load_ntfs_nodes(connection, root)?;
    let old_by_id = old_records
        .iter()
        .cloned()
        .map(|record| (record.id, record))
        .collect::<HashMap<_, _>>();
    let mut new_by_id = old_by_id.clone();
    let mut changed_ids = HashSet::new();
    let mut changed_directories = HashSet::new();
    for change in &changes {
        changed_ids.insert(change.id);
        if change.is_directory()
            || old_by_id
                .get(&change.id)
                .is_some_and(MftRecord::is_directory)
        {
            changed_directories.insert(change.id);
        }
        if change.reason & USN_REASON_FILE_DELETE != 0 {
            new_by_id.remove(&change.id);
        } else if change.reason & USN_REASON_RENAME_OLD_NAME == 0 {
            new_by_id.insert(change.id, change.clone());
        }
    }
    let old_affected = affected_file_ids(&old_by_id, &changed_ids, &changed_directories);
    let new_affected = affected_file_ids(&new_by_id, &changed_ids, &changed_directories);

    let tx = connection.transaction()?;
    {
        let mut delete_node =
            tx.prepare_cached("DELETE FROM ntfs_nodes WHERE root = ?1 AND file_id = ?2")?;
        let mut upsert_node = tx.prepare_cached(
            "INSERT INTO ntfs_nodes(root, file_id, parent_id, name, attributes, usn)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(root, file_id) DO UPDATE SET
               parent_id=excluded.parent_id, name=excluded.name,
               attributes=excluded.attributes, usn=excluded.usn",
        )?;
        for change in &changes {
            if change.reason & USN_REASON_FILE_DELETE != 0 {
                delete_node.execute(params![root, change.id.as_bytes().as_slice()])?;
            } else if change.reason & USN_REASON_RENAME_OLD_NAME == 0 {
                upsert_node.execute(params![
                    root,
                    change.id.as_bytes().as_slice(),
                    change.parent_id.as_bytes().as_slice(),
                    change.name,
                    change.attributes,
                    change.usn,
                ])?;
            }
        }
        let mut delete_file =
            tx.prepare_cached("DELETE FROM files WHERE root = ?1 AND file_id = ?2")?;
        for id in &old_affected {
            delete_file.execute(params![root, id.as_bytes().as_slice()])?;
        }
    }
    tx.commit()?;

    let records = new_by_id.into_values().collect::<Vec<_>>();
    resolve_mft_files_in_batches(root, records, NTFS_RESOLVE_BATCH, |entries| {
        let files = entries
            .into_iter()
            .filter(|entry| new_affected.contains(&entry.id))
            .filter(|entry| !is_excluded(Path::new(&entry.path), exclusions))
            .map(|entry| indexed_mft_entry(root, entry))
            .collect::<Vec<_>>();
        write_batch(connection, &files)
    })?;
    Ok(())
}

#[cfg(windows)]
fn apply_file_usn_changes(
    connection: &mut Connection,
    root: &str,
    exclusions: &[String],
    changes: Vec<MftRecord>,
) -> anyhow::Result<()> {
    const USN_REASON_FILE_DELETE: u32 = 0x0000_0200;
    const USN_REASON_RENAME_OLD_NAME: u32 = 0x0000_1000;
    let mut changed = HashMap::new();
    let tx = connection.transaction()?;
    {
        let mut delete_node =
            tx.prepare_cached("DELETE FROM ntfs_nodes WHERE root = ?1 AND file_id = ?2")?;
        let mut upsert_node = tx.prepare_cached(
            "INSERT INTO ntfs_nodes(root, file_id, parent_id, name, attributes, usn)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(root, file_id) DO UPDATE SET
               parent_id=excluded.parent_id, name=excluded.name,
               attributes=excluded.attributes, usn=excluded.usn",
        )?;
        let mut delete_file =
            tx.prepare_cached("DELETE FROM files WHERE root = ?1 AND file_id = ?2")?;
        for change in changes {
            delete_file.execute(params![root, change.id.as_bytes().as_slice()])?;
            if change.reason & USN_REASON_FILE_DELETE != 0 {
                delete_node.execute(params![root, change.id.as_bytes().as_slice()])?;
                changed.remove(&change.id);
            } else if change.reason & USN_REASON_RENAME_OLD_NAME == 0 {
                upsert_node.execute(params![
                    root,
                    change.id.as_bytes().as_slice(),
                    change.parent_id.as_bytes().as_slice(),
                    change.name,
                    change.attributes,
                    change.usn,
                ])?;
                changed.insert(change.id, change);
            }
        }
    }
    tx.commit()?;

    if changed.is_empty() {
        return Ok(());
    }
    let mut records = load_ntfs_records(connection, root, true)?;
    records.extend(changed.into_values());
    resolve_mft_files_in_batches(root, records, NTFS_RESOLVE_BATCH, |entries| {
        let files = entries
            .into_iter()
            .filter(|entry| !is_excluded(Path::new(&entry.path), exclusions))
            .map(|entry| indexed_mft_entry(root, entry))
            .collect::<Vec<_>>();
        write_batch(connection, &files)
    })?;
    Ok(())
}

#[cfg(windows)]
fn affected_file_ids(
    records: &HashMap<crate::ntfs::FileId, MftRecord>,
    changed_ids: &HashSet<crate::ntfs::FileId>,
    changed_directories: &HashSet<crate::ntfs::FileId>,
) -> HashSet<crate::ntfs::FileId> {
    records
        .values()
        .filter(|record| !record.is_directory())
        .filter(|record| {
            if changed_ids.contains(&record.id) {
                return true;
            }
            let mut current = record.parent_id;
            let mut visited = HashSet::new();
            while visited.insert(current) {
                if changed_directories.contains(&current) {
                    return true;
                }
                let Some(parent) = records.get(&current) else {
                    break;
                };
                if parent.id == parent.parent_id {
                    break;
                }
                current = parent.parent_id;
            }
            false
        })
        .map(|record| record.id)
        .collect()
}

#[cfg(windows)]
fn save_ntfs_volume_state(
    connection: &Connection,
    root: &str,
    volume: char,
    journal: &UsnJournalInfo,
    snapshot_complete: bool,
) -> anyhow::Result<()> {
    let identity = format!(
        "{}:{}",
        volume.to_ascii_uppercase(),
        ntfs_volume_serial(root).unwrap_or(0)
    );
    connection.execute(
        "INSERT INTO search_volumes(
           root, provider, volume_identity, journal_id, next_usn,
           provider_version, schema_version, snapshot_complete
         ) VALUES(?1, 'windowsNtfs', ?2, ?3, ?4, 1, ?5, ?6)
         ON CONFLICT(root) DO UPDATE SET
           provider=excluded.provider, volume_identity=excluded.volume_identity,
           journal_id=excluded.journal_id, next_usn=excluded.next_usn,
           provider_version=excluded.provider_version,
           schema_version=excluded.schema_version,
           snapshot_complete=excluded.snapshot_complete",
        params![
            root,
            identity,
            journal.journal_id.to_le_bytes().as_slice(),
            journal.next_usn,
            SCHEMA_VERSION,
            snapshot_complete,
        ],
    )?;
    Ok(())
}

#[cfg(windows)]
#[derive(Debug)]
struct NtfsVolumeState {
    volume_identity: String,
    journal_id: u64,
    next_usn: i64,
    snapshot_complete: bool,
}

#[cfg(windows)]
fn load_ntfs_volume_state(
    connection: &Connection,
    root: &str,
) -> anyhow::Result<Option<NtfsVolumeState>> {
    connection
        .query_row(
            "SELECT volume_identity, journal_id, next_usn, snapshot_complete
             FROM search_volumes
             WHERE root = ?1 AND provider = 'windowsNtfs'
               AND provider_version = 1 AND schema_version = ?2",
            params![root, SCHEMA_VERSION],
            |row| {
                let journal_id = row.get::<_, Vec<u8>>(1)?;
                let journal_id: [u8; 8] = journal_id.try_into().map_err(|_| {
                    rusqlite::Error::InvalidColumnType(
                        1,
                        "journal_id".into(),
                        rusqlite::types::Type::Blob,
                    )
                })?;
                Ok(NtfsVolumeState {
                    volume_identity: row.get(0)?,
                    journal_id: u64::from_le_bytes(journal_id),
                    next_usn: row.get(2)?,
                    snapshot_complete: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

#[cfg(windows)]
struct NtfsCatchUpJob {
    root: String,
    volume: char,
    exclusions: Vec<String>,
    journal: UsnJournalInfo,
    changes: Vec<MftRecord>,
}

#[cfg(windows)]
enum NtfsResumeJob {
    CatchUp(NtfsCatchUpJob),
    Rebuild(NtfsFinalizeJob),
}

#[cfg(windows)]
fn prepare_ntfs_catch_up(
    db_path: &Path,
    root: &str,
    volume: char,
    exclusions: &[String],
) -> anyhow::Result<NtfsCatchUpJob> {
    let connection = open_database(db_path)?;
    let state = load_ntfs_volume_state(&connection, root)?
        .filter(|state| state.snapshot_complete)
        .ok_or_else(|| anyhow::anyhow!("NTFS 快照未完成"))?;
    let identity = format!(
        "{}:{}",
        volume.to_ascii_uppercase(),
        ntfs_volume_serial(root).unwrap_or(0)
    );
    let journal = query_usn_via_service(volume)?;
    if state.volume_identity != identity
        || state.journal_id != journal.journal_id
        || state.next_usn < journal.first_usn
        || state.next_usn > journal.next_usn
    {
        anyhow::bail!("NTFS 卷身份或 USN 断点已失效");
    }
    let mut changes = Vec::new();
    read_usn_via_service(
        volume,
        state.next_usn,
        state.journal_id,
        journal.next_usn,
        |batch| {
            if changes.len().saturating_add(batch.len()) > MAX_USN_REPLAY_RECORDS {
                anyhow::bail!("USN 追赶记录超过有界上限，需要重建该卷");
            }
            changes.extend(batch);
            Ok(())
        },
    )?;
    if usn_changes_require_rebuild(&connection, root, &changes)? {
        anyhow::bail!("USN 包含目录变化，需要全量重建该卷");
    }
    Ok(NtfsCatchUpJob {
        root: root.into(),
        volume,
        exclusions: exclusions.to_vec(),
        journal,
        changes,
    })
}

#[cfg(windows)]
fn usn_changes_require_rebuild(
    connection: &Connection,
    root: &str,
    changes: &[MftRecord],
) -> anyhow::Result<bool> {
    let mut attributes = connection
        .prepare_cached("SELECT attributes FROM ntfs_nodes WHERE root = ?1 AND file_id = ?2")?;
    changes.iter().try_fold(false, |found, change| {
        if found || change.is_directory() {
            return Ok(true);
        }
        let previous = attributes
            .query_row(params![root, change.id.as_bytes().as_slice()], |row| {
                row.get::<_, u32>(0)
            })
            .optional()?;
        Ok(previous.is_some_and(|value| value & FILE_ATTRIBUTE_DIRECTORY != 0))
    })
}

#[cfg(all(windows, test))]
#[derive(Debug)]
struct DirectoryChangeDuringUsnRead;

#[cfg(all(windows, test))]
impl std::fmt::Display for DirectoryChangeDuringUsnRead {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("directory change detected while reading USN batches")
    }
}

#[cfg(all(windows, test))]
impl std::error::Error for DirectoryChangeDuringUsnRead {}

#[cfg(all(windows, test))]
fn collect_usn_changes_until_directory_change<F, R>(
    connection: &Connection,
    root: &str,
    read_batches: F,
) -> anyhow::Result<(Vec<MftRecord>, bool)>
where
    F: FnOnce(&mut dyn FnMut(Vec<MftRecord>) -> anyhow::Result<()>) -> anyhow::Result<R>,
{
    let mut changes = Vec::new();
    let mut on_batch = |batch: Vec<MftRecord>| {
        if usn_changes_require_rebuild(connection, root, &batch)? {
            return Err(anyhow::Error::new(DirectoryChangeDuringUsnRead));
        }
        changes.extend(batch);
        Ok(())
    };
    match read_batches(&mut on_batch) {
        Ok(_) => Ok((changes, false)),
        Err(error)
            if error
                .downcast_ref::<DirectoryChangeDuringUsnRead>()
                .is_some() =>
        {
            Ok((changes, true))
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct PersistenceUsnReplayStopped(&'static str);

#[cfg(windows)]
impl std::fmt::Display for PersistenceUsnReplayStopped {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "persistence USN replay stopped: {}", self.0)
    }
}

#[cfg(windows)]
impl std::error::Error for PersistenceUsnReplayStopped {}

#[cfg(windows)]
fn collect_persistence_usn_changes<F, R>(
    known_directories: &HashSet<crate::ntfs::FileId>,
    max_records: usize,
    max_duration: Duration,
    read_batches: F,
) -> anyhow::Result<(Vec<MftRecord>, Option<&'static str>)>
where
    F: FnOnce(&mut dyn FnMut(Vec<MftRecord>) -> anyhow::Result<()>) -> anyhow::Result<R>,
{
    let started = std::time::Instant::now();
    let mut changes = Vec::new();
    let mut on_batch = |batch: Vec<MftRecord>| {
        if batch
            .iter()
            .any(|change| change.is_directory() || known_directories.contains(&change.id))
        {
            return Err(anyhow::Error::new(PersistenceUsnReplayStopped(
                "directory-change-during-persistence",
            )));
        }
        if changes.len().saturating_add(batch.len()) > max_records
            || started.elapsed() >= max_duration
        {
            return Err(anyhow::Error::new(PersistenceUsnReplayStopped(
                "bounded-usn-replay-limit",
            )));
        }
        changes.extend(batch);
        Ok(())
    };
    match read_batches(&mut on_batch) {
        Ok(_) => Ok((changes, None)),
        Err(error) => match error.downcast_ref::<PersistenceUsnReplayStopped>() {
            Some(stopped) => Ok((changes, Some(stopped.0))),
            None => Err(error),
        },
    }
}

#[cfg(windows)]
fn persistence_usn_range_exceeds_limit(start_usn: i64, target_usn: i64) -> bool {
    target_usn.saturating_sub(start_usn) > PERSISTENCE_USN_REPLAY_MAX_RANGE
}

#[cfg(windows)]
fn apply_ntfs_catch_up(db_path: &Path, job: NtfsCatchUpJob) -> anyhow::Result<()> {
    let mut connection = open_database(db_path)?;
    apply_usn_changes(&mut connection, &job.root, &job.exclusions, job.changes)?;
    save_ntfs_volume_state(&connection, &job.root, job.volume, &job.journal, true)
}

fn scan_folder_roots<S: SearchStatusSink>(
    manager: &Arc<FileSearchManager>,
    app: &S,
    generation: u64,
    config: &SearchConfig,
    roots: &[String],
) -> anyhow::Result<()> {
    let mut connection = open_database(&manager.db_path)?;
    connection.pragma_update(None, "synchronous", "OFF")?;
    connection.pragma_update(None, "cache_size", -65_536)?;
    let mut pending = Vec::with_capacity(SCAN_WRITE_BATCH);
    for root in roots {
        if manager.is_cancelled(generation) {
            return Ok(());
        }
        let fallback_reason = manager
            .status
            .lock()
            .unwrap()
            .providers
            .iter()
            .find(|item| item.root == *root)
            .and_then(|item| item.fallback_reason.clone());
        set_provider_stage(
            manager,
            app,
            root,
            "folderScan",
            "scanning",
            "scanning",
            fallback_reason,
        );
        let root_path = PathBuf::from(root);
        if !root_path.is_dir() || is_excluded(&root_path, &config.exclusions) {
            continue;
        }
        let mut directories = vec![root_path.clone()];
        #[cfg(all(unix, not(target_os = "macos")))]
        let root_device = unix_device(&root_path);
        while let Some(directory) = directories.pop() {
            if manager.is_cancelled(generation) {
                return Ok(());
            }
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(_) => {
                    manager.status.lock().unwrap().skipped_directories += 1;
                    continue;
                }
            };
            for entry in entries.flatten() {
                if manager.is_cancelled(generation) {
                    return Ok(());
                }
                let path = entry.path();
                if is_excluded(&path, &config.exclusions) {
                    continue;
                }
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_dir() {
                    if is_reparse_point(&entry) || is_platform_skipped_directory(&path) {
                        continue;
                    }
                    #[cfg(all(unix, not(target_os = "macos")))]
                    if root_device.is_some() && unix_device(&path) != root_device {
                        continue;
                    }
                    directories.push(path);
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                if let Some(indexed) = indexed_file(&path, root) {
                    pending.push(indexed);
                    let mut status = manager.status.lock().unwrap();
                    if let Some(provider) = status
                        .providers
                        .iter_mut()
                        .find(|provider| provider.root == *root)
                    {
                        provider.discovered_records = provider.discovered_records.saturating_add(1);
                    }
                    status.scanned_files = status
                        .providers
                        .iter()
                        .map(|provider| provider.discovered_records)
                        .sum();
                    let scanned = status.scanned_files;
                    drop(status);
                    if pending.len() >= SCAN_WRITE_BATCH {
                        let searchable = pending.len() as u64;
                        write_batch(&mut connection, &pending)?;
                        manager.index_files(&pending)?;
                        pending.clear();
                        add_provider_progress(manager, app, root, "scanning", 0, searchable);
                    }
                    if scanned % 2048 == 0 {
                        manager.refresh_counts();
                        manager.emit_status(app);
                    }
                }
            }
        }
        if !pending.is_empty() {
            let searchable = pending.len() as u64;
            write_batch(&mut connection, &pending)?;
            manager.index_files(&pending)?;
            pending.clear();
            add_provider_progress(manager, app, root, "scanning", 0, searchable);
        }
    }
    manager.commit_query_index()?;
    for root in roots {
        let fallback_reason = manager
            .status
            .lock()
            .unwrap()
            .providers
            .iter()
            .find(|item| item.root == *root)
            .and_then(|item| item.fallback_reason.clone());
        set_provider_status(manager, app, root, "folderScan", "ready", fallback_reason);
    }
    Ok(())
}

fn apply_event_paths(
    db_path: &Path,
    config: &SearchConfig,
    paths: &[PathBuf],
) -> anyhow::Result<()> {
    let mut connection = open_database(db_path)?;
    let tx = connection.transaction()?;
    for path in paths {
        let Some(root) = containing_root(path, &config.roots) else {
            continue;
        };
        if is_excluded(path, &config.exclusions) || is_platform_skipped_directory(path) {
            continue;
        }
        if path.is_file() {
            if let Some(file) = indexed_file(path, root) {
                upsert_file(&tx, &file)?;
            }
        } else if path.is_dir() {
            upsert_subtree(&tx, path, root, &config.exclusions)?;
        } else {
            let value = path.to_string_lossy();
            let prefix = format!("{}{}", value, std::path::MAIN_SEPARATOR);
            let upper_bound = format!("{prefix}{}", char::MAX);
            tx.execute("DELETE FROM files WHERE path = ?1", params![value.as_ref()])?;
            tx.execute(
                "DELETE FROM files WHERE path >= ?1 AND path < ?2",
                params![prefix, upper_bound],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn collect_event_paths_bounded(receiver: &Receiver<Event>) -> Vec<PathBuf> {
    let started = std::time::Instant::now();
    eprintln!("[search-index] stage=handoff event=start records=0");
    let mut batches = 0;
    let mut event_paths = 0usize;
    let mut pending_paths = Vec::new();
    while event_handoff_should_continue(batches, started.elapsed()) {
        let mut paths = Vec::new();
        loop {
            match receiver.try_recv() {
                Ok(event) => paths.extend(event.paths),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
            if paths.len() >= EVENT_BATCH {
                break;
            }
        }
        if paths.is_empty() {
            break;
        }
        event_paths = event_paths.saturating_add(paths.len());
        pending_paths.extend(paths);
        batches += 1;
    }
    pending_paths.sort();
    pending_paths.dedup();
    eprintln!(
        "[search-index] stage=handoff event=complete batches={batches} records={event_paths} elapsed_ms={}",
        started.elapsed().as_millis()
    );
    pending_paths
}

fn event_handoff_should_continue(batches: usize, elapsed: Duration) -> bool {
    batches < EVENT_HANDOFF_MAX_BATCHES && elapsed < EVENT_HANDOFF_MAX_DURATION
}

fn upsert_subtree(
    connection: &Connection,
    path: &Path,
    root: &str,
    exclusions: &[String],
) -> anyhow::Result<()> {
    let mut directories = vec![path.to_path_buf()];
    #[cfg(all(unix, not(target_os = "macos")))]
    let root_device = unix_device(Path::new(root));
    while let Some(directory) = directories.pop() {
        if is_excluded(&directory, exclusions) || is_platform_skipped_directory(&directory) {
            continue;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if is_excluded(&path, exclusions) {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if is_reparse_point(&entry) || is_platform_skipped_directory(&path) {
                    continue;
                }
                #[cfg(all(unix, not(target_os = "macos")))]
                if root_device.is_some() && unix_device(&path) != root_device {
                    continue;
                }
                directories.push(path);
            } else if file_type.is_file() {
                if let Some(file) = indexed_file(&path, root) {
                    upsert_file(connection, &file)?;
                }
            }
        }
    }
    Ok(())
}

fn containing_root<'a>(path: &Path, roots: &'a [String]) -> Option<&'a str> {
    roots
        .iter()
        .filter(|root| path_is_within(path, Path::new(root)))
        .max_by_key(|root| Path::new(root).components().count())
        .map(String::as_str)
}

#[derive(Debug, Clone, Copy, Default)]
struct DatabaseInitialization {
    query_snapshot_complete: bool,
    persistence_incomplete: bool,
}

#[cfg(test)]
fn initialize_database(path: &Path) -> anyhow::Result<DatabaseInitialization> {
    initialize_database_with_query(path, None)
}

fn initialize_database_with_query(
    path: &Path,
    query_documents: Option<u64>,
) -> anyhow::Result<DatabaseInitialization> {
    let connection = open_database(path)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS files(
           path TEXT PRIMARY KEY NOT NULL,
           name TEXT NOT NULL,
           parent TEXT NOT NULL,
           root TEXT NOT NULL,
           size INTEGER NOT NULL,
           modified_ms INTEGER,
           is_log INTEGER NOT NULL,
           is_archive INTEGER NOT NULL,
           file_id BLOB,
           parent_id BLOB
         );
         DROP INDEX IF EXISTS files_name_idx;
         DROP INDEX IF EXISTS files_modified_idx;
         CREATE TABLE IF NOT EXISTS ntfs_nodes(
           root TEXT NOT NULL,
           file_id BLOB NOT NULL,
           parent_id BLOB NOT NULL,
           name TEXT NOT NULL,
           attributes INTEGER NOT NULL,
           usn INTEGER NOT NULL,
           PRIMARY KEY(root, file_id)
         );
         CREATE INDEX IF NOT EXISTS ntfs_nodes_parent_idx
           ON ntfs_nodes(root, parent_id);
         CREATE TABLE IF NOT EXISTS search_volumes(
           root TEXT PRIMARY KEY NOT NULL,
           provider TEXT NOT NULL,
           volume_identity TEXT NOT NULL,
           journal_id BLOB,
           next_usn INTEGER,
           provider_version INTEGER NOT NULL,
           schema_version INTEGER NOT NULL,
           snapshot_complete INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS search_index_changes(
           path TEXT PRIMARY KEY NOT NULL,
           operation INTEGER NOT NULL
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
           name, path, content='files', content_rowid='rowid',
           tokenize='trigram', detail='none', columnsize=0
         );",
    )?;
    ensure_file_identity_columns(&connection)?;
    let version = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let incomplete = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'bulk_rebuild'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        == Some(1);
    let query_snapshot_marker = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'query_snapshot_complete'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let persisted_files = connection
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, u64>(0))
        .unwrap_or(0);
    let legacy_completed_query = query_snapshot_marker.is_none()
        && incomplete
        && persisted_files > 0
        && query_documents.is_some_and(|documents| documents > 0);
    let query_snapshot_complete = query_snapshot_marker == Some(1) || legacy_completed_query;
    let mut state = DatabaseInitialization::default();
    if version != Some(SCHEMA_VERSION) {
        recreate_index(&connection)?;
        connection.execute(
            "INSERT OR REPLACE INTO metadata(key, value) VALUES('schema_version', ?1)",
            params![SCHEMA_VERSION],
        )?;
    } else if incomplete && query_snapshot_complete {
        reset_persistence(&connection)?;
        state.query_snapshot_complete = true;
        state.persistence_incomplete = true;
    } else if incomplete {
        reset_index(&connection)?;
    } else {
        connection.execute_batch(CREATE_FTS_TRIGGERS)?;
        connection.execute_batch(CREATE_QUERY_CHANGE_TRIGGERS)?;
    }
    connection.execute(
        "INSERT OR IGNORE INTO metadata(key, value) VALUES('fts_ready', 1)",
        [],
    )?;
    if !state.query_snapshot_complete {
        state.query_snapshot_complete = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'query_snapshot_complete'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            == Some(1);
    }
    Ok(state)
}

fn recreate_index(connection: &Connection) -> anyhow::Result<()> {
    connection.execute_batch(DROP_FTS_TRIGGERS)?;
    connection.execute_batch(DROP_QUERY_CHANGE_TRIGGERS)?;
    connection.execute("DELETE FROM files", [])?;
    connection.execute("DELETE FROM ntfs_nodes", [])?;
    connection.execute("DELETE FROM search_volumes", [])?;
    connection.execute("DELETE FROM search_index_changes", [])?;
    connection.execute("DROP TABLE IF EXISTS files_fts", [])?;
    connection.execute_batch(CREATE_FTS_TABLE)?;
    connection.execute_batch(
        "INSERT OR REPLACE INTO metadata(key, value) VALUES('bulk_rebuild', 0);
         INSERT OR REPLACE INTO metadata(key, value) VALUES('fts_ready', 1);
         INSERT OR REPLACE INTO metadata(key, value) VALUES('query_snapshot_complete', 0);",
    )?;
    connection.execute_batch(CREATE_FTS_TRIGGERS)?;
    connection.execute_batch(CREATE_QUERY_CHANGE_TRIGGERS)?;
    Ok(())
}

fn ensure_file_identity_columns(connection: &Connection) -> anyhow::Result<()> {
    let mut statement = connection.prepare("PRAGMA table_info(files)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<HashSet<_>, _>>()?;
    if !columns.contains("file_id") {
        connection.execute("ALTER TABLE files ADD COLUMN file_id BLOB", [])?;
    }
    if !columns.contains("parent_id") {
        connection.execute("ALTER TABLE files ADD COLUMN parent_id BLOB", [])?;
    }
    Ok(())
}

fn reset_index(connection: &Connection) -> anyhow::Result<()> {
    connection.execute_batch(DROP_FTS_TRIGGERS)?;
    connection.execute_batch(DROP_QUERY_CHANGE_TRIGGERS)?;
    connection.execute("DELETE FROM files", [])?;
    connection.execute("DELETE FROM ntfs_nodes", [])?;
    connection.execute("DELETE FROM search_volumes", [])?;
    connection.execute("DELETE FROM search_index_changes", [])?;
    connection.execute_batch(
        "INSERT INTO files_fts(files_fts) VALUES('rebuild');
         INSERT OR REPLACE INTO metadata(key, value) VALUES('bulk_rebuild', 0);
         INSERT OR REPLACE INTO metadata(key, value) VALUES('fts_ready', 1);
         INSERT OR REPLACE INTO metadata(key, value) VALUES('query_snapshot_complete', 0);",
    )?;
    connection.execute_batch(CREATE_FTS_TRIGGERS)?;
    connection.execute_batch(CREATE_QUERY_CHANGE_TRIGGERS)?;
    Ok(())
}

fn reset_persistence(connection: &Connection) -> anyhow::Result<()> {
    connection.execute_batch(DROP_FTS_TRIGGERS)?;
    connection.execute_batch(DROP_QUERY_CHANGE_TRIGGERS)?;
    connection.execute("DELETE FROM search_index_changes", [])?;
    connection.execute_batch(
        "INSERT OR REPLACE INTO metadata(key, value) VALUES('bulk_rebuild', 1);
         INSERT OR REPLACE INTO metadata(key, value) VALUES('fts_ready', 0);
         INSERT OR REPLACE INTO metadata(key, value) VALUES('query_snapshot_complete', 1);",
    )?;
    Ok(())
}

fn prepare_bulk_index(path: &Path, rebuild: bool) -> anyhow::Result<()> {
    let connection = open_database(path)?;
    connection.execute_batch(
        "INSERT OR REPLACE INTO metadata(key, value) VALUES('bulk_rebuild', 1);
         INSERT OR REPLACE INTO metadata(key, value) VALUES('fts_ready', 0);",
    )?;
    connection.execute_batch(DROP_FTS_TRIGGERS)?;
    connection.execute_batch(DROP_QUERY_CHANGE_TRIGGERS)?;
    connection.execute("DELETE FROM search_index_changes", [])?;
    if rebuild {
        connection.execute(
            "INSERT OR REPLACE INTO metadata(key, value) VALUES('query_snapshot_complete', 0)",
            [],
        )?;
        connection.execute("DELETE FROM files", [])?;
        connection.execute("DELETE FROM ntfs_nodes", [])?;
        connection.execute("DELETE FROM search_volumes", [])?;
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn mark_query_snapshot_complete(path: &Path) -> anyhow::Result<()> {
    let connection = open_database(path)?;
    connection.execute(
        "INSERT OR REPLACE INTO metadata(key, value) VALUES('query_snapshot_complete', 1)",
        [],
    )?;
    Ok(())
}

fn finish_bulk_index(path: &Path) -> anyhow::Result<()> {
    let connection = open_database(path)?;
    connection.execute_batch(
        "INSERT OR REPLACE INTO metadata(key, value) VALUES('bulk_rebuild', 0);
         INSERT OR REPLACE INTO metadata(key, value) VALUES('fts_ready', 0);
         INSERT OR REPLACE INTO metadata(key, value) VALUES('query_snapshot_complete', 1);",
    )?;
    connection.execute_batch(CREATE_QUERY_CHANGE_TRIGGERS)?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

fn open_database(path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(15))?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(connection)
}

fn clear_rows(path: &Path) -> anyhow::Result<()> {
    let connection = open_database(path)?;
    reset_index(&connection)?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

fn write_batch(connection: &mut Connection, files: &[IndexedFile]) -> anyhow::Result<()> {
    write_files(connection, files, true)
}

fn write_files(
    connection: &mut Connection,
    files: &[IndexedFile],
    update_existing: bool,
) -> anyhow::Result<()> {
    let tx = connection.transaction()?;
    write_file_rows(&tx, files, update_existing)?;
    tx.commit()?;
    Ok(())
}

fn write_file_rows(
    connection: &Connection,
    files: &[IndexedFile],
    update_existing: bool,
) -> anyhow::Result<()> {
    for chunk in files.chunks(3_000) {
        let placeholders = (0..chunk.len())
            .map(|_| "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .collect::<Vec<_>>()
            .join(",");
        let conflict = if update_existing {
            "ON CONFLICT(path) DO UPDATE SET
               name=excluded.name, parent=excluded.parent, root=excluded.root,
               size=excluded.size, modified_ms=excluded.modified_ms,
               is_log=excluded.is_log, is_archive=excluded.is_archive,
               file_id=excluded.file_id, parent_id=excluded.parent_id"
        } else {
            "ON CONFLICT(path) DO NOTHING"
        };
        let sql = format!(
            "INSERT INTO files(
               path, name, parent, root, size, modified_ms, is_log, is_archive, file_id, parent_id
             ) VALUES {placeholders}
             {conflict}"
        );
        let mut values = Vec::<rusqlite::types::Value>::with_capacity(chunk.len() * 10);
        for file in chunk {
            values.push(file.path.clone().into());
            values.push(file.name.clone().into());
            values.push(String::new().into());
            values.push(file.root.clone().into());
            values.push((file.size as i64).into());
            values.push(
                file.modified_ms
                    .map(|value| rusqlite::types::Value::Integer(value as i64))
                    .unwrap_or(rusqlite::types::Value::Null),
            );
            values.push((file.is_log as i64).into());
            values.push((file.is_archive as i64).into());
            values.push(
                file.file_id
                    .map(|value| rusqlite::types::Value::Blob(value.to_vec()))
                    .unwrap_or(rusqlite::types::Value::Null),
            );
            values.push(
                file.parent_id
                    .map(|value| rusqlite::types::Value::Blob(value.to_vec()))
                    .unwrap_or(rusqlite::types::Value::Null),
            );
        }
        connection.execute(&sql, rusqlite::params_from_iter(values))?;
    }
    Ok(())
}

fn upsert_file(connection: &Connection, file: &IndexedFile) -> anyhow::Result<()> {
    connection.execute(
        "INSERT INTO files(
           path, name, parent, root, size, modified_ms, is_log, is_archive, file_id, parent_id
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(path) DO UPDATE SET
           name=excluded.name, parent=excluded.parent, root=excluded.root,
           size=excluded.size, modified_ms=excluded.modified_ms,
           is_log=excluded.is_log, is_archive=excluded.is_archive,
           file_id=excluded.file_id, parent_id=excluded.parent_id",
        params![
            file.path,
            file.name,
            "",
            file.root,
            file.size,
            file.modified_ms,
            file.is_log,
            file.is_archive,
            file.file_id.as_ref().map(|id| id.as_slice()),
            file.parent_id.as_ref().map(|id| id.as_slice()),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
fn query_fts(
    connection: &Connection,
    terms: &[String],
    filter_sql: &str,
    offset: u32,
    limit: u32,
) -> anyhow::Result<(Vec<SearchResultItem>, u64)> {
    let expression = terms
        .iter()
        .flat_map(|term| {
            let chars = term.chars().collect::<Vec<_>>();
            chars
                .windows(3)
                .map(|window| {
                    let token = window.iter().collect::<String>();
                    format!("\"{}\"", token.replace('"', "\"\""))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let validation = terms
        .iter()
        .enumerate()
        .map(|(index, _)| {
            format!(
                " AND instr(lower(f.name || ' ' || f.path), ?{}) > 0",
                index + 2
            )
        })
        .collect::<String>();
    let from = format!(
        " FROM files_fts s JOIN files f ON f.rowid = s.rowid
          WHERE files_fts MATCH ?1{validation}{filter_sql}"
    );
    let mut count_values = Vec::<rusqlite::types::Value>::with_capacity(terms.len() + 1);
    count_values.push(expression.clone().into());
    count_values.extend(terms.iter().cloned().map(Into::into));
    let total = connection.query_row(
        &format!("SELECT COUNT(*){from}"),
        rusqlite::params_from_iter(count_values),
        |row| row.get(0),
    )?;
    let exact_index = terms.len() + 2;
    let prefix_index = exact_index + 1;
    let limit_index = exact_index + 2;
    let offset_index = exact_index + 3;
    let sql = format!(
        "SELECT f.path, f.name, f.parent, f.size, f.modified_ms, f.is_log, f.is_archive{from}
         ORDER BY CASE WHEN lower(f.name) = ?{exact_index}
                       THEN 0 WHEN lower(f.name) LIKE ?{prefix_index} THEN 1 ELSE 2 END,
                  length(f.name), f.name COLLATE NOCASE, f.path COLLATE NOCASE
         LIMIT ?{limit_index} OFFSET ?{offset_index}"
    );
    let first = terms.first().cloned().unwrap_or_default();
    let mut values = Vec::<rusqlite::types::Value>::with_capacity(terms.len() + 5);
    values.push(expression.into());
    values.extend(terms.iter().cloned().map(Into::into));
    values.push(first.clone().into());
    values.push(format!("{first}%").into());
    values.push(i64::from(limit).into());
    values.push(i64::from(offset).into());
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(values), result_item)?;
    Ok((rows.collect::<Result<Vec<_>, _>>()?, total))
}

fn query_like(
    connection: &Connection,
    terms: &[String],
    filter_sql: &str,
    offset: u32,
    limit: u32,
) -> anyhow::Result<(Vec<SearchResultItem>, u64)> {
    let clauses = terms
        .iter()
        .enumerate()
        .map(|(index, _)| format!("instr(lower(f.name || ' ' || f.path), ?{}) > 0", index + 1))
        .collect::<Vec<_>>()
        .join(" AND ");
    let where_sql = format!(" WHERE {clauses}{filter_sql}");
    let values = terms
        .iter()
        .map(|term| term as &dyn rusqlite::ToSql)
        .collect::<Vec<_>>();
    let total = connection.query_row(
        &format!("SELECT COUNT(*) FROM files f{where_sql}"),
        values.as_slice(),
        |row| row.get(0),
    )?;
    let sql = format!(
        "SELECT f.path, f.name, f.parent, f.size, f.modified_ms, f.is_log, f.is_archive
         FROM files f{where_sql}
         ORDER BY length(f.name), f.name COLLATE NOCASE, f.path COLLATE NOCASE
         LIMIT {limit} OFFSET {offset}"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(values.as_slice(), result_item)?;
    Ok((rows.collect::<Result<Vec<_>, _>>()?, total))
}

fn result_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchResultItem> {
    let path = row.get::<_, String>(0)?;
    let stored_parent = row.get::<_, String>(2)?;
    let is_log = row.get::<_, bool>(5)?;
    let is_archive = row.get::<_, bool>(6)?;
    Ok(SearchResultItem {
        parent: if stored_parent.is_empty() {
            Path::new(&path)
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            stored_parent
        },
        path,
        name: row.get(1)?,
        size: row.get(3)?,
        modified_ms: row.get(4)?,
        readable: false,
        content_type: content_type_for_name(&row.get::<_, String>(1)?).into(),
        kind: if is_archive {
            "archive".into()
        } else if is_log {
            "log".into()
        } else {
            "file".into()
        },
        is_log,
        is_archive,
    })
}

fn indexed_file(path: &Path, root: &str) -> Option<IndexedFile> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    let is_archive = is_archive_name(&name);
    Some(IndexedFile {
        path: path.to_string_lossy().into_owned(),
        is_log: is_log_name(&name),
        is_archive,
        name,
        root: root.into(),
        size: metadata.len(),
        modified_ms: metadata.modified().ok().and_then(system_time_ms),
        file_id: None,
        parent_id: None,
    })
}

fn enrich_visible_metadata(items: &mut [SearchResultItem]) {
    if items.is_empty() {
        return;
    }
    let workers = items.len().min(METADATA_WORKERS_MAX);
    let chunk_size = (items.len() + workers - 1) / workers;
    std::thread::scope(|scope| {
        for chunk in items.chunks_mut(chunk_size) {
            scope.spawn(move || {
                for item in chunk {
                    let Ok(metadata) = fs::metadata(&item.path) else {
                        continue;
                    };
                    if !metadata.is_file() {
                        continue;
                    }
                    item.size = metadata.len();
                    item.modified_ms = metadata.modified().ok().and_then(system_time_ms);
                    item.readable = fs::File::open(&item.path).is_ok();
                }
            });
        }
    });
}

fn content_type_for_name(name: &str) -> &'static str {
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "zip" => "application/zip",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "tar" => "application/x-tar",
        "gz" | "tgz" => "application/gzip",
        "bz2" | "tbz2" => "application/x-bzip2",
        "xz" | "txz" => "application/x-xz",
        "log" | "txt" | "out" | "err" | "trace" | "json" | "xml" | "yaml" | "yml" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn system_time_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

fn is_excluded(path: &Path, exclusions: &[String]) -> bool {
    exclusions
        .iter()
        .any(|excluded| path_is_within(path, Path::new(excluded)))
}

#[cfg(windows)]
fn path_is_within(path: &Path, ancestor: &Path) -> bool {
    let path = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let ancestor = ancestor
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    path == ancestor || path.starts_with(&format!("{ancestor}\\"))
}

#[cfg(not(windows))]
fn path_is_within(path: &Path, ancestor: &Path) -> bool {
    path.starts_with(ancestor)
}

#[cfg(target_os = "macos")]
fn is_platform_skipped_directory(path: &Path) -> bool {
    [
        "/Volumes",
        "/System/Volumes",
        "/Network",
        "/dev",
        "/net",
        "/home",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
}

#[cfg(not(target_os = "macos"))]
fn is_platform_skipped_directory(_path: &Path) -> bool {
    false
}

fn normalize_unique_paths(paths: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter_map(|path| {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                return None;
            }
            let normalized = PathBuf::from(trimmed).to_string_lossy().into_owned();
            path_key(&normalized).and_then(|key| seen.insert(key).then_some(normalized))
        })
        .collect()
}

fn path_key(path: &str) -> Option<String> {
    if path.is_empty() {
        None
    } else if cfg!(windows) {
        Some(path.replace('/', "\\").to_lowercase())
    } else {
        Some(path.into())
    }
}

fn read_config(path: &Path) -> Option<SearchConfig> {
    let bytes = fs::read(path).ok()?;
    let mut config = serde_json::from_slice::<SearchConfig>(&bytes).ok()?;
    config.version = search_config_version();
    config.roots = normalize_unique_paths(config.roots);
    config.exclusions = normalize_unique_paths(config.exclusions);
    Some(config)
}

fn write_config(path: &Path, config: &SearchConfig) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(config)?)?;
    if let Err(error) = replace_file(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn database_size(path: &Path) -> u64 {
    [path.to_path_buf(), path.with_extension("sqlite3-wal")]
        .iter()
        .filter_map(|path| fs::metadata(path).ok().map(|metadata| metadata.len()))
        .sum()
}

fn planned_provider_statuses(roots: &[String]) -> Vec<SearchProviderStatus> {
    roots
        .iter()
        .map(|root| SearchProviderStatus {
            root: root.clone(),
            provider: planned_provider(root).into(),
            phase: "pending".into(),
            stage: "pending".into(),
            discovered_records: 0,
            searchable_files: 0,
            started_ms: None,
            elapsed_ms: 0,
            stage_started_ms: None,
            stage_elapsed_ms: 0,
            completed_ms: None,
            fallback_reason: None,
        })
        .collect()
}

#[cfg(windows)]
fn planned_provider(root: &str) -> &'static str {
    if ntfs_volume_letter(root).is_some() {
        "windowsNtfs"
    } else {
        "folderScan"
    }
}

#[cfg(not(windows))]
fn planned_provider(_root: &str) -> &'static str {
    "folderScan"
}

#[cfg(windows)]
fn ntfs_volume_letter(root: &str) -> Option<char> {
    ntfs_volume_details(root).map(|(letter, _)| letter)
}

#[cfg(windows)]
fn ntfs_volume_serial(root: &str) -> Option<u32> {
    ntfs_volume_details(root).map(|(_, serial)| serial)
}

#[cfg(windows)]
fn ntfs_volume_details(root: &str) -> Option<(char, u32)> {
    const DRIVE_FIXED: u32 = 3;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDriveTypeW(root_path_name: *const u16) -> u32;
        fn GetVolumeInformationW(
            root_path_name: *const u16,
            volume_name_buffer: *mut u16,
            volume_name_size: u32,
            volume_serial_number: *mut u32,
            maximum_component_length: *mut u32,
            file_system_flags: *mut u32,
            file_system_name_buffer: *mut u16,
            file_system_name_size: u32,
        ) -> i32;
    }
    let normalized = root.replace('/', "\\");
    let trimmed = normalized.trim_end_matches('\\');
    let bytes = trimmed.as_bytes();
    if bytes.len() != 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let letter = (bytes[0] as char).to_ascii_uppercase();
    let root = format!("{letter}:\\");
    let wide = root
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe { GetDriveTypeW(wide.as_ptr()) } != DRIVE_FIXED {
        return None;
    }
    let mut file_system = [0_u16; 32];
    let mut serial = 0_u32;
    let success = unsafe {
        GetVolumeInformationW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            file_system.as_mut_ptr(),
            file_system.len() as u32,
        )
    };
    if success == 0 {
        return None;
    }
    let length = file_system
        .iter()
        .position(|word| *word == 0)
        .unwrap_or(file_system.len());
    String::from_utf16_lossy(&file_system[..length])
        .eq_ignore_ascii_case("NTFS")
        .then_some((letter, serial))
}

#[cfg(windows)]
fn local_fixed_roots() -> Vec<String> {
    const DRIVE_FIXED: u32 = 3;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLogicalDrives() -> u32;
        fn GetDriveTypeW(root_path_name: *const u16) -> u32;
    }
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter_map(|index| {
            if mask & (1 << index) == 0 {
                return None;
            }
            let letter = (b'A' + index as u8) as char;
            let root = format!("{letter}:\\");
            let wide = root
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            (unsafe { GetDriveTypeW(wide.as_ptr()) } == DRIVE_FIXED).then_some(root)
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn local_fixed_roots() -> Vec<String> {
    vec!["/".into()]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn local_fixed_roots() -> Vec<String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .map(|path| vec![path.to_string_lossy().into_owned()])
        .unwrap_or_else(|| vec!["/".into()])
}

#[cfg(windows)]
fn is_reparse_point(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    entry
        .metadata()
        .map(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        .unwrap_or(true)
}

#[cfg(not(windows))]
fn is_reparse_point(_entry: &fs::DirEntry) -> bool {
    false
}

#[cfg(all(unix, not(target_os = "macos")))]
fn unix_device(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|metadata| metadata.dev())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "logcrate-search-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn search_feature_defaults_to_enabled() {
        let config = SearchConfig::default();
        assert!(config.enabled);
        assert_eq!(config.version, search_config_version());
    }

    #[test]
    fn invalid_search_config_safely_falls_back_to_enabled() {
        let directory = test_directory("invalid-config");
        fs::write(directory.join("file-search.json"), b"not-json").unwrap();
        let preferences = SearchPreferenceStore::new(directory.clone());
        assert_eq!(
            preferences.feature_state(false),
            SearchFeatureState {
                current_enabled: false,
                next_launch_enabled: true,
            }
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    fn readable_root_falls_back_from_service_failure_to_folder_provider() {
        let directory = test_directory("service-folder-fallback");
        let root_path = directory.join("root");
        let data_path = directory.join("data");
        fs::create_dir_all(&root_path).unwrap();
        fs::write(root_path.join("fallback.log"), b"fallback").unwrap();
        let root = root_path.to_string_lossy().into_owned();
        let manager = Arc::new(FileSearchManager::new(data_path));
        {
            let mut status = manager.status.lock().unwrap();
            status.roots = vec![root.clone()];
            status.providers = planned_provider_statuses(&status.roots);
        }
        let failure = anyhow::Error::new(ServiceFailure {
            code: crate::ntfs::ipc::ServiceFailureCode::ProtocolMismatch,
            message: "protocol mismatch".into(),
        });
        let reason = compatible_provider_fallback_reason(&root, &failure)
            .unwrap()
            .expect("服务软件故障且卷根可读时必须允许兼容 provider 降级");
        assert!(reason.contains("自动降级到兼容目录扫描"));

        set_provider_stage(
            &manager,
            &NoopSearchStatusSink,
            &root,
            "folderScan",
            "fallback",
            "fallback",
            Some(reason),
        );
        clear_root_for_compatible_provider(&manager.db_path, &root).unwrap();
        let config = SearchConfig {
            version: search_config_version(),
            enabled: true,
            roots: vec![root.clone()],
            exclusions: vec![directory.join("data").to_string_lossy().into_owned()],
        };
        scan_folder_roots(
            &manager,
            &NoopSearchStatusSink,
            0,
            &config,
            std::slice::from_ref(&root),
        )
        .unwrap();

        assert_eq!(indexed_root_count(&manager.db_path, &root), 1);
        let status = manager.status();
        assert_eq!(status.providers[0].provider, "folderScan");
        assert_eq!(status.providers[0].phase, "ready");
        assert!(status.providers[0]
            .fallback_reason
            .as_deref()
            .unwrap()
            .contains("protocol mismatch"));

        let missing_root = directory.join("missing").to_string_lossy().into_owned();
        assert!(compatible_provider_fallback_reason(&missing_root, &failure).is_err());
        let non_service_failure = anyhow::anyhow!("query index merge failed");
        assert!(
            compatible_provider_fallback_reason(&missing_root, &non_service_failure)
                .unwrap()
                .is_none()
        );

        drop(manager);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupted_database_is_quarantined_for_rebuild() {
        let directory = test_directory("quarantine-database");
        let database = directory.join("file-search.sqlite3");
        fs::write(&database, b"corrupt").unwrap();
        fs::write(directory.join("file-search.sqlite3-wal"), b"wal").unwrap();
        quarantine_database(&database).unwrap();
        assert!(!database.exists());
        assert!(fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains("corrupt-")));
        initialize_database_with_query(&database, None).unwrap();
        let connection = open_database(&database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT value FROM metadata WHERE key = 'schema_version'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            SCHEMA_VERSION
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn legacy_search_config_is_migrated_without_enabling_search() {
        let directory = test_directory("legacy-config");
        let path = directory.join("file-search.json");
        fs::write(
            &path,
            br#"{"enabled":false,"roots":["D:\\"],"exclusions":[]}"#,
        )
        .unwrap();
        let config = read_config(&path).unwrap();
        assert_eq!(config.version, search_config_version());
        assert!(!config.enabled);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn enabled_preference_is_separate_from_current_process_state() {
        let directory = test_directory("feature-state");
        let preferences = SearchPreferenceStore::new(directory.clone());
        preferences.set_enabled(true).unwrap();
        assert_eq!(
            preferences.feature_state(false),
            SearchFeatureState {
                current_enabled: false,
                next_launch_enabled: true,
            }
        );
        let persisted = read_config(&directory.join("file-search.json")).unwrap();
        assert!(persisted.enabled);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn failed_preference_write_restores_previous_value() {
        let directory = test_directory("preference-write-failure");
        let preferences = SearchPreferenceStore::new(directory.clone());
        fs::create_dir(&preferences.config_path).unwrap();
        assert!(preferences.set_enabled(false).is_err());
        assert!(preferences.feature_state(false).next_launch_enabled);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn default_preference_read_does_not_create_search_storage() {
        let parent = test_directory("lightweight-preference");
        let search_dir = parent.join("search");
        let preferences = SearchPreferenceStore::new(search_dir.clone());
        assert!(preferences.config().enabled);
        assert!(!search_dir.exists());
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn read_connection_does_not_renegotiate_wal_while_a_writer_is_active() {
        let directory = test_directory("concurrent-read");
        let database = directory.join("index.sqlite3");
        initialize_database(&database).unwrap();

        let writer = open_database(&database).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        writer
            .execute(
                "INSERT OR REPLACE INTO metadata(key, value) VALUES('writer_active', 1)",
                [],
            )
            .unwrap();

        let reader = open_database(&database).unwrap();
        let journal_mode = reader
            .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .unwrap();
        let file_count = reader
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, u64>(0))
            .unwrap();
        assert_eq!(journal_mode, "wal");
        assert_eq!(file_count, 0);

        writer.execute_batch("ROLLBACK").unwrap();
        drop(reader);
        drop(writer);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn visible_metadata_is_enriched_without_reading_file_contents() {
        let directory = test_directory("visible-metadata");
        let path = directory.join("service.log");
        fs::write(&path, b"metadata-only").unwrap();
        let mut items = vec![SearchResultItem {
            path: path.to_string_lossy().into_owned(),
            name: "service.log".into(),
            parent: directory.to_string_lossy().into_owned(),
            kind: "log".into(),
            size: 0,
            modified_ms: None,
            readable: false,
            content_type: content_type_for_name("service.log").into(),
            is_log: true,
            is_archive: false,
        }];

        enrich_visible_metadata(&mut items);

        assert_eq!(items[0].size, 13);
        assert!(items[0].modified_ms.is_some());
        assert!(items[0].readable);
        assert_eq!(items[0].content_type, "text/plain");
        assert_eq!(content_type_for_name("bundle.zip"), "application/zip");
        assert_eq!(
            content_type_for_name("unknown.bin"),
            "application/octet-stream"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn query_snapshot_switch_is_atomic_and_recovers_interrupted_rebuild() {
        let directory = test_directory("atomic-query-snapshot");
        let manager = FileSearchManager::new(directory.clone());
        manager
            .query_index
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .add_batch(&[SearchIndexEntry {
                path: "D:\\logs\\stable-old.log".into(),
                name: "stable-old.log".into(),
                scope_key: String::new(),
                is_log: true,
                is_archive: false,
            }])
            .unwrap();
        manager.commit_query_index().unwrap();
        manager.query_index_ready.store(true, Ordering::Release);

        manager.begin_query_index_bulk().unwrap();
        manager
            .index_files(&[IndexedFile {
                path: "D:\\logs\\replacement-new.log".into(),
                name: "replacement-new.log".into(),
                root: "D:\\".into(),
                size: 1,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }])
            .unwrap();
        let (_, old_total) = manager
            .query_tantivy(&["stable".into()], "log", 0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(old_total, 1, "旧快照应在新快照完成前保持可查询");
        manager.commit_query_index().unwrap();
        manager.status.lock().unwrap().phase = "scanning".into();
        let (_, partial_total) = manager
            .query_tantivy(&["replacement".into()], "log", 0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(partial_total, 1, "重建期间应查询已提交的 staging 结果");
        manager.finish_query_index_bulk().unwrap();
        let (_, new_total) = manager
            .query_tantivy(&["replacement".into()], "log", 0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(new_total, 1);
        let (_, old_total) = manager
            .query_tantivy(&["stable".into()], "log", 0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(old_total, 0);

        manager.begin_query_index_bulk().unwrap();
        drop(manager);
        let recovered = FileSearchManager::new(directory.clone());
        recovered.query_index_ready.store(true, Ordering::Release);
        let (_, new_total) = recovered
            .query_tantivy(&["replacement".into()], "log", 0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(new_total, 1, "中断重建后应恢复上一份完整快照");
        assert!(!query_index_staging_path(&recovered.query_index_path).exists());
        drop(recovered);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn query_index_filesystem_errors_include_stage_paths_and_use_bounded_retry() {
        let attempts = AtomicU64::new(0);
        let source = Path::new("source.next");
        let destination = Path::new("active");
        let result = retry_query_index_fs(
            "switch-staging-to-active",
            source,
            Some(destination),
            || {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                if cfg!(windows) && attempt < 2 {
                    Err(std::io::Error::from_raw_os_error(5))
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            if cfg!(windows) { 3 } else { 1 }
        );

        let error = retry_query_index_fs::<(), _>(
            "switch-active-to-previous",
            source,
            Some(destination),
            || Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing")),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("stage=switch-active-to-previous"));
        assert!(error.contains("source.next"));
        assert!(error.contains("active"));
    }

    #[test]
    fn query_snapshot_switch_tolerates_concurrent_queries() {
        let directory = test_directory("concurrent-query-switch");
        let manager = FileSearchManager::new(directory.clone());
        manager
            .query_index
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .add_batch(&[SearchIndexEntry {
                path: "C:\\old.log".into(),
                name: "old.log".into(),
                scope_key: String::new(),
                is_log: true,
                is_archive: false,
            }])
            .unwrap();
        manager.commit_query_index().unwrap();
        manager.query_index_ready.store(true, Ordering::Release);
        manager.begin_query_index_bulk().unwrap();
        manager
            .index_files(&[IndexedFile {
                path: "D:\\new.log".into(),
                name: "new.log".into(),
                root: "D:\\".into(),
                size: 0,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }])
            .unwrap();
        manager.commit_query_index().unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        std::thread::scope(|scope| {
            let stop_query = Arc::clone(&stop);
            let manager_query = Arc::clone(&manager);
            scope.spawn(move || {
                while !stop_query.load(Ordering::Relaxed) {
                    let _ = manager_query.query_tantivy(&["old".into()], "", 0, 10);
                }
            });
            manager.finish_query_index_bulk().unwrap();
            stop.store(true, Ordering::Relaxed);
        });
        let (_, total) = manager
            .query_tantivy(&["new".into()], "", 0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(total, 1);
        drop(manager);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn query_index_recovery_promotes_previous_snapshot_when_active_is_missing() {
        let directory = test_directory("previous-query-recovery");
        let active = directory.join("index");
        let previous = query_index_previous_path(&active);
        let mut index = SearchIndex::open(&previous).unwrap();
        index
            .add_batch(&[SearchIndexEntry {
                path: "C:\\restored.log".into(),
                name: "restored.log".into(),
                scope_key: String::new(),
                is_log: true,
                is_archive: false,
            }])
            .unwrap();
        index.commit().unwrap();
        drop(index);

        recover_query_index_directories(&active).unwrap();
        assert!(active.exists());
        assert!(!previous.exists());
        let index = SearchIndex::open(&active).unwrap();
        assert_eq!(index.search(&["restored".into()], "", 0, 10).unwrap().1, 1);
        drop(index);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_staging_switch_rolls_the_previous_snapshot_back() {
        let directory = test_directory("query-switch-rollback");
        let manager = FileSearchManager::new(directory.clone());
        manager
            .query_index
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .add_batch(&[SearchIndexEntry {
                path: "C:\\stable.log".into(),
                name: "stable.log".into(),
                scope_key: String::new(),
                is_log: true,
                is_archive: false,
            }])
            .unwrap();
        manager.commit_query_index().unwrap();
        manager.query_index_ready.store(true, Ordering::Release);

        let error = manager
            .activate_staged_query_index()
            .unwrap_err()
            .to_string();
        assert!(error.contains("stage=switch-staging-to-active"));
        assert!(manager.query_index_path.exists());
        assert!(!query_index_previous_path(&manager.query_index_path).exists());
        let (_, total) = manager
            .query_tantivy(&["stable".into()], "", 0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(total, 1);
        drop(manager);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn query_switch_diagnostics_include_operation_and_directory_state() {
        let directory = test_directory("query-switch-diagnostics");
        let manager = FileSearchManager::new(directory.clone());
        *manager.operation_snapshot.lock().unwrap() = Some(IndexOperationSnapshot {
            operation_id: "search-diagnostic-test".into(),
            generation: 7,
            started_ms: 1,
            query_ready_ms: None,
            persistence_completed_ms: None,
            event_handoff_completed_ms: None,
            converged_ms: None,
            final_phase: "scanning".into(),
            error: None,
            scopes: Vec::new(),
        });
        let diagnostic = manager.operation_diagnostic_context();
        assert!(diagnostic.contains("operation_id=search-diagnostic-test"));
        assert!(diagnostic.contains("active=true"));
        assert!(diagnostic.contains("staging=false"));
        assert!(diagnostic.contains("previous=false"));
        assert!(diagnostic.contains("concurrent_queries=0"));
        drop(manager);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_switch_preserves_old_query_and_reports_ui_error_context() {
        let directory = test_directory("query-switch-ui-error");
        let manager = FileSearchManager::new(directory.clone());
        manager
            .query_index
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .add_batch(&[SearchIndexEntry {
                path: "C:\\stable.log".into(),
                name: "stable.log".into(),
                scope_key: "C:\\".into(),
                is_log: true,
                is_archive: false,
            }])
            .unwrap();
        manager.commit_query_index().unwrap();
        manager.query_index_ready.store(true, Ordering::Release);
        let error = manager
            .activate_staged_query_index()
            .unwrap_err()
            .to_string();
        let (_, total) = manager
            .query_tantivy(&["stable".into()], "", 0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(total, 1);
        let sink = RecordingSearchStatusSink::default();
        manager.finish_with_error(
            &sink,
            manager.generation.load(Ordering::Acquire),
            anyhow::anyhow!(error),
        );
        let status = sink.0.lock().unwrap().last().cloned().unwrap();
        assert_eq!(status.phase, "error");
        assert!(status.error.unwrap().contains("operation_id="));
        drop(manager);
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn query_snapshot_switch_handles_external_file_occupancy() {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        for round in 0..3 {
            let directory = test_directory(&format!("query-switch-external-occupancy-{round}"));
            let manager = FileSearchManager::new(directory.clone());
            manager
                .query_index
                .lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .add_batch(&[SearchIndexEntry {
                    path: "C:\\old.log".into(),
                    name: "old.log".into(),
                    scope_key: "C:\\".into(),
                    is_log: true,
                    is_archive: false,
                }])
                .unwrap();
            manager.commit_query_index().unwrap();
            let staging_path = query_index_staging_path(&manager.query_index_path);
            let mut staging = SearchIndex::open(&staging_path).unwrap();
            staging
                .add_batch(&[SearchIndexEntry {
                    path: "C:\\new.log".into(),
                    name: "new.log".into(),
                    scope_key: "C:\\".into(),
                    is_log: true,
                    is_archive: false,
                }])
                .unwrap();
            staging.commit().unwrap();
            staging.close().unwrap();
            let held_path = fs::read_dir(&manager.query_index_path)
                .unwrap()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| path.is_file())
                .expect("active Tantivy index must contain a file");
            let held = OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&held_path)
                .unwrap();
            let first_attempt = manager.activate_staged_query_index();
            drop(held);
            if first_attempt.is_err() {
                manager.activate_staged_query_index().unwrap();
            }
            assert!(manager.query_index_path.exists());
            drop(manager);
            fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn provider_switch_deduplicates_paths_and_removes_stale_results() {
        let directory = test_directory("provider-switch");
        let file_path = directory.join("service.log");
        fs::write(&file_path, b"first").unwrap();
        let manager = FileSearchManager::new(directory.clone());
        manager.query_index_ready.store(true, Ordering::Release);
        let root = directory.to_string_lossy().into_owned();
        let make_file = |size| IndexedFile {
            path: file_path.to_string_lossy().into_owned(),
            name: "service.log".into(),
            root: root.clone(),
            size,
            modified_ms: None,
            is_log: true,
            is_archive: false,
            file_id: None,
            parent_id: None,
        };

        let mut connection = open_database(&manager.db_path).unwrap();
        write_batch(&mut connection, &[make_file(5)]).unwrap();
        manager.index_files(&[make_file(5)]).unwrap();
        manager.commit_query_index().unwrap();
        write_batch(&mut connection, &[make_file(9)]).unwrap();
        manager.index_files(&[make_file(9)]).unwrap();
        manager.commit_query_index().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            1
        );
        drop(connection);
        let (_, total) = manager
            .query_tantivy(&["service".into()], "log", 0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(total, 1);

        fs::remove_file(&file_path).unwrap();
        let config = SearchConfig {
            version: search_config_version(),
            enabled: true,
            roots: vec![root],
            exclusions: Vec::new(),
        };
        apply_event_paths(&manager.db_path, &config, &[file_path]).unwrap();
        manager.drain_query_index_changes().unwrap();
        assert!(manager
            .query_tantivy(&["service".into()], "log", 0, 10)
            .unwrap()
            .is_none());
        drop(manager);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn sqlite_fallback_matches_name_path_filters_and_pagination() {
        let directory = test_directory("query");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let mut connection = open_database(&db).unwrap();
        let files = vec![
            IndexedFile {
                path: "D:\\work\\logs\\server-error.log".into(),
                name: "server-error.log".into(),
                root: "D:\\".into(),
                size: 12,
                modified_ms: Some(2),
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            },
            IndexedFile {
                path: "D:\\download\\server-backup.zip".into(),
                name: "server-backup.zip".into(),
                root: "D:\\".into(),
                size: 24,
                modified_ms: Some(1),
                is_log: false,
                is_archive: true,
                file_id: None,
                parent_id: None,
            },
        ];
        write_batch(&mut connection, &files).unwrap();
        let (items, total) = query_fts(&connection, &["server".into()], "", 0, 1).unwrap();
        assert_eq!(total, 2);
        assert_eq!(items.len(), 1);
        let (items, total) = query_fts(
            &connection,
            &["server".into()],
            " AND f.is_archive = 1",
            0,
            10,
        )
        .unwrap();
        assert_eq!(total, 1);
        assert_eq!(items[0].kind, "archive");
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn short_terms_use_case_insensitive_substring_matching() {
        let directory = test_directory("short");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let mut connection = open_database(&db).unwrap();
        write_batch(
            &mut connection,
            &[IndexedFile {
                path: "/Users/test/logs/api.log".into(),
                name: "api.log".into(),
                root: "/".into(),
                size: 4,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }],
        )
        .unwrap();
        let (items, total) = query_like(&connection, &["ap".into()], "", 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(items[0].name, "api.log");
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn exclusions_are_normalized_and_match_descendants() {
        let root = test_directory("exclude");
        let excluded = root.join("private");
        fs::create_dir_all(excluded.join("nested")).unwrap();
        let normalized = normalize_unique_paths(vec![
            excluded.to_string_lossy().into_owned(),
            excluded.to_string_lossy().into_owned(),
        ]);
        assert_eq!(normalized.len(), 1);
        assert!(is_excluded(&excluded.join("nested/file.log"), &normalized));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn ntfs_volume_workers_are_bounded_by_service_capacity() {
        assert_eq!(ntfs_volume_worker_count(0), 0);
        assert_eq!(ntfs_volume_worker_count(2), 2);
        assert_eq!(ntfs_volume_worker_count(10), NTFS_VOLUME_WORKERS_MAX);
    }

    #[cfg(windows)]
    #[test]
    fn volume_scheduler_consumes_results_in_completion_order_and_isolates_failures() {
        let consumed = Mutex::new(Vec::new());
        let completed = run_ntfs_volume_tasks(
            vec![
                ("slow".into(), 'S'),
                ("fast".into(), 'F'),
                ("failed".into(), 'X'),
            ],
            &|root, _| {
                if root == "slow" {
                    std::thread::sleep(Duration::from_millis(40));
                }
                if root == "failed" {
                    anyhow::bail!("simulated volume failure");
                }
                Ok(root)
            },
            &|| false,
            |root, result| {
                consumed.lock().unwrap().push((root, result.is_ok()));
                Ok(())
            },
        )
        .unwrap();

        assert!(completed);
        let consumed = consumed.into_inner().unwrap();
        assert_eq!(consumed.len(), 3);
        assert_eq!(consumed[0].0, "fast");
        assert!(consumed.iter().any(|(root, ok)| root == "slow" && *ok));
        assert!(consumed.iter().any(|(root, ok)| root == "failed" && !ok));
    }

    #[cfg(windows)]
    #[test]
    fn cancelled_volume_scheduler_waits_for_started_workers_to_exit() {
        struct ActiveGuard(Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for ActiveGuard {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }

        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_signal = Arc::clone(&cancelled);
        let trigger = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            cancel_signal.store(true, Ordering::SeqCst);
        });
        let worker_active = Arc::clone(&active);
        let completed = run_ntfs_volume_tasks(
            vec![("C:\\".into(), 'C'), ("D:\\".into(), 'D')],
            &move |_, _| {
                worker_active.fetch_add(1, Ordering::SeqCst);
                let _guard = ActiveGuard(Arc::clone(&worker_active));
                std::thread::sleep(Duration::from_millis(50));
                Ok(())
            },
            &|| cancelled.load(Ordering::SeqCst),
            |_, _| Ok(()),
        )
        .unwrap();
        trigger.join().unwrap();

        assert!(!completed);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn provider_progress_aggregates_real_per_volume_counts() {
        let config = SearchConfig {
            roots: vec!["C:\\".into(), "D:\\".into()],
            ..SearchConfig::default()
        };
        let mut status = SearchStatus::disabled(&config);
        status.phase = "scanning".into();
        status.providers[0].phase = "scanning".into();
        status.providers[0].stage = "enumeratingMft".into();
        status.providers[0].discovered_records = 3_080_000;
        status.providers[1].phase = "scanning".into();
        status.providers[1].stage = "readingUsn".into();
        status.providers[1].discovered_records = 0;

        refresh_provider_elapsed(&mut status);

        assert_eq!(status.scanned_files, 3_080_000);
        assert_eq!(status.providers[0].stage, "enumeratingMft");
        assert_eq!(status.providers[1].stage, "readingUsn");
        assert_eq!(status.providers[1].discovered_records, 0);
    }

    #[test]
    fn initializing_status_is_available_before_manager_creation() {
        let config = SearchConfig {
            enabled: true,
            roots: vec!["C:\\".into()],
            ..SearchConfig::default()
        };

        let status = SearchStatus::initializing(&config);

        assert_eq!(status.phase, "scanning");
        assert_eq!(status.roots, config.roots);
        assert_eq!(status.providers.len(), 1);
        assert_eq!(status.indexed_files, 0);
    }

    #[test]
    fn provider_stage_timing_resets_and_terminal_values_are_frozen() {
        let config = SearchConfig {
            roots: vec!["C:\\".into()],
            ..SearchConfig::default()
        };
        let mut status = SearchStatus::disabled(&config);

        update_provider_stage_at(
            &mut status,
            "C:\\",
            "windowsNtfs",
            "scanning",
            "connecting",
            None,
            100,
        );
        update_provider_stage_at(
            &mut status,
            "C:\\",
            "windowsNtfs",
            "scanning",
            "connecting",
            None,
            150,
        );
        assert_eq!(status.providers[0].elapsed_ms, 50);
        assert_eq!(status.providers[0].stage_elapsed_ms, 50);

        update_provider_stage_at(
            &mut status,
            "C:\\",
            "windowsNtfs",
            "scanning",
            "enumeratingMft",
            None,
            200,
        );
        assert_eq!(status.providers[0].elapsed_ms, 100);
        assert_eq!(status.providers[0].stage_elapsed_ms, 0);

        update_provider_stage_at(
            &mut status,
            "C:\\",
            "windowsNtfs",
            "ready",
            "persisting",
            None,
            250,
        );
        assert_eq!(status.providers[0].elapsed_ms, 150);
        assert_eq!(status.providers[0].stage_elapsed_ms, 0);
        assert_eq!(status.providers[0].completed_ms, None);
        update_provider_stage_at(
            &mut status,
            "C:\\",
            "windowsNtfs",
            "ready",
            "ready",
            None,
            400,
        );
        assert_eq!(status.providers[0].elapsed_ms, 300);
        assert_eq!(status.providers[0].completed_ms, Some(400));
        let frozen = status.providers[0].clone();
        refresh_provider_elapsed(&mut status);
        assert_eq!(status.providers[0].elapsed_ms, frozen.elapsed_ms);
        assert_eq!(
            status.providers[0].stage_elapsed_ms,
            frozen.stage_elapsed_ms
        );
    }

    #[test]
    fn global_searchable_count_is_the_sum_of_provider_snapshots() {
        let config = SearchConfig {
            roots: vec!["C:\\".into(), "D:\\".into()],
            ..SearchConfig::default()
        };
        let mut status = SearchStatus::disabled(&config);
        status.providers[0].discovered_records = 3_080_000;
        status.providers[0].searchable_files = 2_529_000;
        status.providers[1].discovered_records = 2_798_000;
        status.providers[1].searchable_files = 2_515_000;

        refresh_provider_totals(&mut status);

        assert_eq!(status.scanned_files, 5_878_000);
        assert_eq!(status.indexed_files, 5_044_000);
    }

    #[test]
    fn event_handoff_is_bounded_by_batches_and_wall_time() {
        assert!(event_handoff_should_continue(0, Duration::ZERO));
        assert!(!event_handoff_should_continue(
            EVENT_HANDOFF_MAX_BATCHES,
            Duration::ZERO
        ));
        assert!(!event_handoff_should_continue(
            0,
            EVENT_HANDOFF_MAX_DURATION
        ));
    }

    #[test]
    fn watcher_skips_access_and_directory_metadata_noise() {
        use notify::event::{AccessKind, DataChange, MetadataKind, ModifyKind, RenameMode};

        let directory = test_directory("watcher-event-filter");
        let file = directory.join("changed.log");
        fs::write(&file, b"changed").unwrap();
        assert!(!event_path_requires_reconcile(
            &notify::EventKind::Access(AccessKind::Any),
            &file,
        ));
        assert!(!event_path_requires_reconcile(
            &notify::EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
            &directory,
        ));
        assert!(event_path_requires_reconcile(
            &notify::EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            &file,
        ));
        assert!(event_path_requires_reconcile(
            &notify::EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &directory,
        ));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn bounded_event_handoff_leaves_remaining_events_for_the_worker() {
        let directory = test_directory("event-handoff");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let event_count = EVENT_BATCH * EVENT_HANDOFF_MAX_BATCHES + 1;
        for index in 0..event_count {
            sender
                .send(
                    Event::new(notify::EventKind::Any)
                        .add_path(directory.join(format!("missing-{index}.log"))),
                )
                .unwrap();
        }

        let handoff_paths = collect_event_paths_bounded(&receiver);

        assert_eq!(handoff_paths.len(), EVENT_BATCH * EVENT_HANDOFF_MAX_BATCHES);
        assert!(receiver.try_recv().is_ok());
        drop(sender);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn ready_state_hands_later_events_to_the_resident_consumer_without_loss() {
        let directory = test_directory("event-ready-handoff");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let root = directory.to_string_lossy().into_owned();
        let config = SearchConfig {
            enabled: true,
            roots: vec![root],
            exclusions: Vec::new(),
            ..SearchConfig::default()
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let (first_sender, first_receiver) = sync_channel(0);
        let (continue_sender, continue_receiver) = sync_channel(0);
        let producer_directory = directory.clone();
        let producer = std::thread::spawn(move || {
            for index in 0..10 {
                let path = producer_directory.join(format!("before-ready-{index}.log"));
                fs::write(&path, b"ready").unwrap();
                sender
                    .send(Event::new(notify::EventKind::Any).add_path(path))
                    .unwrap();
            }
            first_sender.send(()).unwrap();
            continue_receiver.recv().unwrap();
            for index in 0..600 {
                let path = producer_directory.join(format!("after-ready-{index}.log"));
                fs::write(&path, b"resident").unwrap();
                sender
                    .send(Event::new(notify::EventKind::Any).add_path(path))
                    .unwrap();
            }
        });

        first_receiver.recv().unwrap();
        let handoff_paths = collect_event_paths_bounded(&receiver);
        apply_event_paths(&db, &config, &handoff_paths).unwrap();
        let mut status = SearchStatus::disabled(&config);
        status.phase = "ready".into();
        assert_eq!(status.phase, "ready");
        continue_sender.send(()).unwrap();

        producer.join().unwrap();
        while let Ok(first) = receiver.recv_timeout(Duration::from_millis(10)) {
            let mut paths = first.paths;
            while let Ok(event) = receiver.try_recv() {
                paths.extend(event.paths);
                if paths.len() >= EVENT_BATCH {
                    break;
                }
            }
            apply_event_paths(&db, &config, &paths).unwrap();
        }
        let connection = open_database(&db).unwrap();
        let count = connection
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, u64>(0))
            .unwrap();
        assert_eq!(count, 610);
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    fn windows_scope_matching_ignores_case_and_mixed_separators() {
        assert!(path_is_within(
            Path::new("C:\\Users\\Alice\\Logs\\app.log"),
            Path::new("c:/users/alice/logs"),
        ));
        assert!(!path_is_within(
            Path::new("C:\\Users\\Alice\\Logs-old\\app.log"),
            Path::new("c:/users/alice/logs"),
        ));
    }

    #[cfg(windows)]
    #[test]
    fn usn_directory_rename_and_delete_update_only_affected_files() {
        use crate::ntfs::{FileId, FILE_ATTRIBUTE_DIRECTORY};

        let directory = test_directory("usn-update");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let mut connection = open_database(&db).unwrap();
        let records = vec![
            MftRecord {
                id: FileId::from_u64(5),
                parent_id: FileId::from_u64(5),
                name: ".".into(),
                attributes: FILE_ATTRIBUTE_DIRECTORY,
                reason: 0,
                usn: 1,
            },
            MftRecord {
                id: FileId::from_u64(10),
                parent_id: FileId::from_u64(5),
                name: "Logs".into(),
                attributes: FILE_ATTRIBUTE_DIRECTORY,
                reason: 0,
                usn: 2,
            },
            MftRecord {
                id: FileId::from_u64(11),
                parent_id: FileId::from_u64(10),
                name: "app.log".into(),
                attributes: 0,
                reason: 0,
                usn: 3,
            },
        ];
        replace_ntfs_nodes(&mut connection, "C:\\", &records).unwrap();
        write_batch(
            &mut connection,
            &[IndexedFile {
                path: "C:\\Logs\\app.log".into(),
                name: "app.log".into(),
                root: "C:\\".into(),
                size: 0,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: Some(FileId::from_u64(11).as_bytes()),
                parent_id: Some(FileId::from_u64(10).as_bytes()),
            }],
        )
        .unwrap();

        apply_usn_changes(
            &mut connection,
            "C:\\",
            &[],
            vec![MftRecord {
                id: FileId::from_u64(10),
                parent_id: FileId::from_u64(5),
                name: "Renamed".into(),
                attributes: FILE_ATTRIBUTE_DIRECTORY,
                reason: 0x2000,
                usn: 4,
            }],
        )
        .unwrap();
        let path = connection
            .query_row("SELECT path FROM files", [], |row| row.get::<_, String>(0))
            .unwrap();
        assert_eq!(path, "C:\\Renamed\\app.log");

        apply_usn_changes(
            &mut connection,
            "C:\\",
            &[],
            vec![MftRecord {
                id: FileId::from_u64(11),
                parent_id: FileId::from_u64(10),
                name: "app.log".into(),
                attributes: 0,
                reason: 0x200,
                usn: 5,
            }],
        )
        .unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            0
        );
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    fn directory_usn_changes_select_full_volume_rebuild() {
        use crate::ntfs::{FileId, FILE_ATTRIBUTE_DIRECTORY};

        let directory = test_directory("usn-strategy");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let mut connection = open_database(&db).unwrap();
        let records = vec![
            MftRecord {
                id: FileId::from_u64(10),
                parent_id: FileId::from_u64(5),
                name: "Logs".into(),
                attributes: FILE_ATTRIBUTE_DIRECTORY,
                reason: 0,
                usn: 1,
            },
            MftRecord {
                id: FileId::from_u64(11),
                parent_id: FileId::from_u64(10),
                name: "app.log".into(),
                attributes: 0,
                reason: 0,
                usn: 2,
            },
        ];
        replace_ntfs_nodes(&mut connection, "C:\\", &records).unwrap();

        let file_change = MftRecord {
            reason: 0x100,
            usn: 3,
            ..records[1].clone()
        };
        assert!(!usn_changes_require_rebuild(&connection, "C:\\", &[file_change]).unwrap());

        let renamed_directory_reported_without_attributes = MftRecord {
            attributes: 0,
            reason: 0x2000,
            usn: 4,
            ..records[0].clone()
        };
        assert!(usn_changes_require_rebuild(
            &connection,
            "C:\\",
            &[renamed_directory_reported_without_attributes]
        )
        .unwrap());
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    fn directory_change_in_first_usn_batch_stops_reading_later_batches() {
        use crate::ntfs::{FileId, FILE_ATTRIBUTE_DIRECTORY};

        let directory = test_directory("usn-directory-early-stop");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let mut connection = open_database(&db).unwrap();
        let directory_record = MftRecord {
            id: FileId::from_u64(10),
            parent_id: FileId::from_u64(5),
            name: "Logs".into(),
            attributes: FILE_ATTRIBUTE_DIRECTORY,
            reason: 0,
            usn: 1,
        };
        replace_ntfs_nodes(
            &mut connection,
            "C:\\",
            std::slice::from_ref(&directory_record),
        )
        .unwrap();

        let mut batches_requested = 0;
        let (changes, directory_change) = collect_usn_changes_until_directory_change(
            &connection,
            "C:\\",
            |on_batch| -> anyhow::Result<()> {
                batches_requested += 1;
                on_batch(vec![MftRecord {
                    attributes: 0,
                    reason: 0x2000,
                    usn: 2,
                    ..directory_record
                }])?;
                batches_requested += 1;
                on_batch(vec![MftRecord {
                    id: FileId::from_u64(11),
                    parent_id: FileId::from_u64(10),
                    name: "never-read.log".into(),
                    attributes: 0,
                    reason: 0x100,
                    usn: 3,
                }])
            },
        )
        .unwrap();

        assert!(directory_change);
        assert!(changes.is_empty());
        assert_eq!(batches_requested, 1);
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    fn persistence_usn_replay_limit_stops_later_batches_and_requests_watcher_reconcile() {
        use crate::ntfs::FileId;

        let directory = test_directory("usn-persistence-limit");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let mut batches_requested = 0;
        let known_directories = HashSet::new();
        let (changes, reason) = collect_persistence_usn_changes(
            &known_directories,
            0,
            Duration::from_secs(30),
            |on_batch| -> anyhow::Result<()> {
                batches_requested += 1;
                on_batch(vec![MftRecord {
                    id: FileId::from_u64(11),
                    parent_id: FileId::from_u64(5),
                    name: "changed.log".into(),
                    attributes: 0,
                    reason: 0x100,
                    usn: 1,
                }])?;
                batches_requested += 1;
                on_batch(Vec::new())
            },
        )
        .unwrap();

        assert!(changes.is_empty());
        assert_eq!(reason, Some("bounded-usn-replay-limit"));
        assert_eq!(batches_requested, 1);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    fn persistence_usn_classifies_attribute_less_known_directory_without_database_reads() {
        use crate::ntfs::FileId;

        let directory_id = FileId::from_u64(10);
        let known_directories = HashSet::from([directory_id]);
        let (changes, reason) = collect_persistence_usn_changes(
            &known_directories,
            MAX_USN_REPLAY_RECORDS,
            Duration::from_secs(30),
            |on_batch| -> anyhow::Result<()> {
                on_batch(vec![MftRecord {
                    id: directory_id,
                    parent_id: FileId::from_u64(5),
                    name: "Renamed".into(),
                    attributes: 0,
                    reason: 0x2000,
                    usn: 1,
                }])
            },
        )
        .unwrap();

        assert!(changes.is_empty());
        assert_eq!(reason, Some("directory-change-during-persistence"));
    }

    #[cfg(windows)]
    #[test]
    fn persistence_usn_range_is_rejected_before_starting_an_unbounded_ipc_read() {
        assert!(!persistence_usn_range_exceeds_limit(100, 100));
        assert!(persistence_usn_range_exceeds_limit(100, 101));
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_file_usn_changes_use_the_fast_path_without_rewriting_other_files() {
        use crate::ntfs::{FileId, FILE_ATTRIBUTE_DIRECTORY};

        let directory = test_directory("usn-file-fast-path");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let mut connection = open_database(&db).unwrap();
        let records = vec![
            MftRecord {
                id: FileId::from_u64(5),
                parent_id: FileId::from_u64(5),
                name: ".".into(),
                attributes: FILE_ATTRIBUTE_DIRECTORY,
                reason: 0,
                usn: 1,
            },
            MftRecord {
                id: FileId::from_u64(10),
                parent_id: FileId::from_u64(5),
                name: "Logs".into(),
                attributes: FILE_ATTRIBUTE_DIRECTORY,
                reason: 0,
                usn: 2,
            },
            MftRecord {
                id: FileId::from_u64(11),
                parent_id: FileId::from_u64(10),
                name: "old.log".into(),
                attributes: 0,
                reason: 0,
                usn: 3,
            },
            MftRecord {
                id: FileId::from_u64(12),
                parent_id: FileId::from_u64(10),
                name: "untouched.log".into(),
                attributes: 0,
                reason: 0,
                usn: 4,
            },
        ];
        replace_ntfs_nodes(&mut connection, "C:\\", &records).unwrap();
        let files = records[2..]
            .iter()
            .map(|record| IndexedFile {
                path: format!("C:\\Logs\\{}", record.name),
                name: record.name.clone(),
                root: "C:\\".into(),
                size: 0,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: Some(record.id.as_bytes()),
                parent_id: Some(record.parent_id.as_bytes()),
            })
            .collect::<Vec<_>>();
        write_batch(&mut connection, &files).unwrap();

        apply_usn_changes(
            &mut connection,
            "C:\\",
            &[],
            vec![MftRecord {
                name: "renamed.log".into(),
                reason: 0x2000,
                usn: 5,
                ..records[2].clone()
            }],
        )
        .unwrap();

        let paths = connection
            .prepare("SELECT path FROM files ORDER BY path")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            paths,
            vec!["C:\\Logs\\renamed.log", "C:\\Logs\\untouched.log"]
        );
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn runtime_config_excludes_the_search_database_directory() {
        let data_dir = test_directory("internal-exclusion");
        let manager = FileSearchManager::new(data_dir.clone());
        let config = manager.runtime_config();
        assert!(is_excluded(&manager.db_path, &config.exclusions));
        drop(manager);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn full_event_queue_marks_the_index_scope_dirty() {
        let (sender, _receiver) = sync_channel(1);
        let dirty = AtomicBool::new(false);
        enqueue_event(&sender, &dirty, Event::new(notify::EventKind::Any));
        enqueue_event(&sender, &dirty, Event::new(notify::EventKind::Any));
        assert!(dirty.load(Ordering::Relaxed));
    }

    #[test]
    fn generation_and_pause_flags_cancel_stale_scans() {
        let data_dir = test_directory("cancel");
        let manager = FileSearchManager::new(data_dir.clone());
        let generation = manager.generation.load(Ordering::Relaxed);
        assert!(!manager.is_cancelled(generation));
        manager.generation.fetch_add(1, Ordering::SeqCst);
        assert!(manager.is_cancelled(generation));
        let current = manager.generation.load(Ordering::Relaxed);
        manager.cancel.store(true, Ordering::SeqCst);
        assert!(manager.is_cancelled(current));
        drop(manager);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn cancellation_waits_for_the_active_operation_before_returning() {
        let data_dir = test_directory("cancel-waits-operation");
        let manager = FileSearchManager::new(data_dir.clone());
        let operation_manager = Arc::clone(&manager);
        let (locked_sender, locked_receiver) = sync_channel(0);
        let (release_sender, release_receiver) = sync_channel(0);
        let holder = std::thread::spawn(move || {
            let _operation = operation_manager.operation.lock().unwrap();
            locked_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
        });
        locked_receiver.recv().unwrap();

        let cancel_manager = Arc::clone(&manager);
        let (cancelled_sender, cancelled_receiver) = sync_channel(0);
        let canceller = std::thread::spawn(move || {
            cancel_manager.cancel_and_wait();
            cancelled_sender.send(()).unwrap();
        });
        assert!(cancelled_receiver
            .recv_timeout(Duration::from_millis(30))
            .is_err());
        release_sender.send(()).unwrap();
        cancelled_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        holder.join().unwrap();
        canceller.join().unwrap();
        assert!(manager.cancel.load(Ordering::SeqCst));
        drop(manager);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn directory_events_add_rename_and_delete_subtrees() {
        let root = test_directory("events");
        let db = root.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let incoming = root.join("incoming");
        fs::create_dir_all(&incoming).unwrap();
        let original = incoming.join("server.log");
        fs::write(&original, b"one").unwrap();
        let config = SearchConfig {
            version: search_config_version(),
            enabled: true,
            roots: vec![root.to_string_lossy().into_owned()],
            exclusions: Vec::new(),
        };

        apply_event_paths(&db, &config, std::slice::from_ref(&incoming)).unwrap();
        let connection = open_database(&db).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            1
        );
        drop(connection);

        let renamed = incoming.join("renamed.log");
        fs::rename(&original, &renamed).unwrap();
        apply_event_paths(&db, &config, &[original, renamed.clone()]).unwrap();
        let connection = open_database(&db).unwrap();
        let names = connection
            .prepare("SELECT name FROM files ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(names, vec!["renamed.log"]);
        drop(connection);

        fs::remove_dir_all(&incoming).unwrap();
        apply_event_paths(&db, &config, &[incoming]).unwrap();
        let connection = open_database(&db).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            0
        );
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn schema_version_change_rebuilds_external_content_index() {
        let directory = test_directory("schema");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let mut connection = open_database(&db).unwrap();
        write_batch(
            &mut connection,
            &[IndexedFile {
                path: "D:\\logs\\obsolete.log".into(),
                name: "obsolete.log".into(),
                root: "D:\\".into(),
                size: 1,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }],
        )
        .unwrap();
        connection
            .execute(
                "UPDATE metadata SET value = 0 WHERE key = 'schema_version'",
                [],
            )
            .unwrap();
        drop(connection);

        initialize_database(&db).unwrap();
        let connection = open_database(&db).unwrap();
        let (items, total) = query_fts(&connection, &["obsolete".into()], "", 0, 10).unwrap();
        assert!(items.is_empty());
        assert_eq!(total, 0);
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn bulk_load_keeps_legacy_fts_disabled_after_tantivy_finalization() {
        let directory = test_directory("bulk");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        prepare_bulk_index(&db, true).unwrap();
        let mut connection = open_database(&db).unwrap();
        write_batch(
            &mut connection,
            &[IndexedFile {
                path: "D:\\logs\\during-build.log".into(),
                name: "during-build.log".into(),
                root: "D:\\".into(),
                size: 1,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }],
        )
        .unwrap();
        let (_, like_total) = query_like(&connection, &["during".into()], "", 0, 10).unwrap();
        let (_, fts_total) = query_fts(&connection, &["during".into()], "", 0, 10).unwrap();
        assert_eq!(like_total, 1);
        assert_eq!(fts_total, 0);
        drop(connection);

        finish_bulk_index(&db).unwrap();
        let connection = open_database(&db).unwrap();
        let (_, fts_total) = query_fts(&connection, &["during".into()], "", 0, 10).unwrap();
        assert_eq!(fts_total, 0);
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn missing_tantivy_index_is_rebuilt_from_existing_database() {
        let directory = test_directory("tantivy-recovery");
        let db = directory.join("file-search.sqlite3");
        let root = local_fixed_roots()
            .into_iter()
            .next()
            .expect("test platform must expose a default search root");
        let recoverable_path = Path::new(&root)
            .join("logs")
            .join("recoverable-debug.log")
            .to_string_lossy()
            .into_owned();
        initialize_database(&db).unwrap();
        let mut connection = open_database(&db).unwrap();
        write_batch(
            &mut connection,
            &[IndexedFile {
                path: recoverable_path.clone(),
                name: "recoverable-debug.log".into(),
                root,
                size: 1,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }],
        )
        .unwrap();
        drop(connection);

        let manager = FileSearchManager::new(directory.clone());
        assert!(!manager.query_index_ready.load(Ordering::Acquire));
        manager.ensure_query_index_matches_database().unwrap();
        assert!(manager.query_index_ready.load(Ordering::Acquire));
        let (items, total) = manager
            .query_tantivy(&["recoverable".into()], "log", 0, 20)
            .unwrap()
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(items[0].path, recoverable_path);
        drop(manager);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn query_snapshot_scope_count_mismatch_is_rejected() {
        let directory = test_directory("scope-count-mismatch");
        let db = directory.join("file-search.sqlite3");
        initialize_database(&db).unwrap();
        let manager = FileSearchManager::new(directory.clone());
        let mut connection = open_database(&db).unwrap();
        write_batch(
            &mut connection,
            &[IndexedFile {
                path: "C:\\logs\\expected.log".into(),
                name: "expected.log".into(),
                root: "C:\\".into(),
                size: 1,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }],
        )
        .unwrap();
        drop(connection);
        let mut index = manager.query_index.lock().unwrap().take().unwrap();
        index
            .add_batch(&[SearchIndexEntry {
                path: "D:\\logs\\wrong.log".into(),
                name: "wrong.log".into(),
                scope_key: "D:\\".into(),
                is_log: true,
                is_archive: false,
            }])
            .unwrap();
        index.commit().unwrap();
        let error = manager.validate_query_index_scopes(&index).unwrap_err();
        assert!(error.to_string().contains("scope count mismatch"));
        drop(index);
        drop(manager);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn interrupted_bulk_load_is_discarded_on_next_initialization() {
        let directory = test_directory("bulk-recovery");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        prepare_bulk_index(&db, true).unwrap();
        let mut connection = open_database(&db).unwrap();
        write_batch(
            &mut connection,
            &[IndexedFile {
                path: "D:\\logs\\incomplete.log".into(),
                name: "incomplete.log".into(),
                root: "D:\\".into(),
                size: 1,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }],
        )
        .unwrap();
        drop(connection);

        initialize_database(&db).unwrap();
        let connection = open_database(&db).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            0
        );
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn completed_query_snapshot_survives_interrupted_persistence() {
        let directory = test_directory("query-snapshot-recovery");
        let db = directory.join("file-search.sqlite3");
        initialize_database(&db).unwrap();
        let mut query_index =
            SearchIndex::open(&directory.join("file-search-orange-gpl-v1")).unwrap();
        query_index.begin_bulk().unwrap();
        query_index
            .add_batch(&[
                SearchIndexEntry {
                    path: "C:\\logs\\debug.log".into(),
                    name: "debug.log".into(),
                    scope_key: String::new(),
                    is_log: true,
                    is_archive: false,
                },
                SearchIndexEntry {
                    path: "D:\\logs\\debug.log".into(),
                    name: "debug.log".into(),
                    scope_key: String::new(),
                    is_log: true,
                    is_archive: false,
                },
            ])
            .unwrap();
        query_index.finish_bulk().unwrap();
        drop(query_index);

        prepare_bulk_index(&db, true).unwrap();
        let mut connection = open_database(&db).unwrap();
        write_batch(
            &mut connection,
            &[IndexedFile {
                path: "D:\\logs\\debug.log".into(),
                name: "debug.log".into(),
                root: "D:\\".into(),
                size: 0,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }],
        )
        .unwrap();
        drop(connection);
        mark_query_snapshot_complete(&db).unwrap();

        let state = initialize_database(&db).unwrap();
        assert!(state.query_snapshot_complete);
        assert!(state.persistence_incomplete);
        let manager = FileSearchManager::new(directory.clone());
        assert!(manager.query_index_ready.load(Ordering::Acquire));
        assert!(manager.persistence_recovery.load(Ordering::Acquire));
        let (items, _) = manager
            .query_tantivy(&["debug.log".into()], "", 0, 20)
            .unwrap()
            .unwrap();
        assert_eq!(items.len(), 2);
        drop(manager);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn legacy_interrupted_persistence_preserves_nonempty_query_index() {
        let directory = test_directory("legacy-query-snapshot-recovery");
        let db = directory.join("file-search.sqlite3");
        initialize_database(&db).unwrap();
        prepare_bulk_index(&db, true).unwrap();
        let mut connection = open_database(&db).unwrap();
        write_batch(
            &mut connection,
            &[IndexedFile {
                path: "D:\\logs\\debug.log".into(),
                name: "debug.log".into(),
                root: "D:\\".into(),
                size: 0,
                modified_ms: None,
                is_log: true,
                is_archive: false,
                file_id: None,
                parent_id: None,
            }],
        )
        .unwrap();
        connection
            .execute(
                "DELETE FROM metadata WHERE key = 'query_snapshot_complete'",
                [],
            )
            .unwrap();
        drop(connection);

        let state = initialize_database_with_query(&db, Some(2)).unwrap();
        assert!(state.query_snapshot_complete);
        assert!(state.persistence_incomplete);
        let connection = open_database(&db).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            1
        );
        drop(connection);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn subtree_scan_does_not_follow_symbolic_link_directories() {
        use std::os::unix::fs::symlink;

        let root = test_directory("symlink-root");
        let outside = test_directory("symlink-outside");
        fs::write(root.join("inside.log"), b"inside").unwrap();
        fs::write(outside.join("outside.log"), b"outside").unwrap();
        symlink(&outside, root.join("linked")).unwrap();
        let db = root.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let connection = open_database(&db).unwrap();
        upsert_subtree(&connection, &root, &root.to_string_lossy(), &[]).unwrap();
        let names = connection
            .prepare("SELECT name FROM files WHERE name LIKE '%.log' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(names, vec!["inside.log"]);
        drop(connection);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    #[ignore = "million-path release performance baseline"]
    fn million_path_search_performance_baseline() {
        let directory = test_directory("million");
        let db = directory.join("search.sqlite3");
        let mut query_index = SearchIndex::open(&directory.join("tantivy")).unwrap();
        query_index.begin_bulk().unwrap();
        initialize_database(&db).unwrap();
        let started = std::time::Instant::now();
        prepare_bulk_index(&db, true).unwrap();
        let mut connection = open_database(&db).unwrap();
        connection
            .pragma_update(None, "synchronous", "OFF")
            .unwrap();
        connection
            .pragma_update(None, "cache_size", -65_536)
            .unwrap();
        for start in (0..1_000_000).step_by(SCAN_WRITE_BATCH) {
            let end = (start + SCAN_WRITE_BATCH).min(1_000_000);
            let files = (start..end)
                .map(|index| IndexedFile {
                    path: format!(
                        "D:\\logs\\service-{}\\server-error-{index}.log",
                        index % 500
                    ),
                    name: format!("server-error-{index}.log"),
                    root: "D:\\".into(),
                    size: index as u64,
                    modified_ms: Some(index as u64),
                    is_log: true,
                    is_archive: false,
                    file_id: None,
                    parent_id: None,
                })
                .collect::<Vec<_>>();
            write_batch(&mut connection, &files).unwrap();
            query_index
                .add_batch(&files.iter().map(search_index_entry).collect::<Vec<_>>())
                .unwrap();
        }
        drop(connection);
        query_index.finish_bulk().unwrap();
        finish_bulk_index(&db).unwrap();
        let build_elapsed = started.elapsed();
        let query_started = std::time::Instant::now();
        let (_, total) = query_index
            .search(&["server".into(), "error-999".into()], "", 0, 100)
            .unwrap();
        println!(
            "million path index: build={build_elapsed:?}, query={:?}, sqlite_bytes={}, documents={}, matches={total}",
            query_started.elapsed(),
            database_size(&db),
            query_index.num_docs(),
        );
        assert!(total > 0);
        drop(query_index);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed LogCrate Index Service and a local NTFS C volume"]
    fn windows_ntfs_end_to_end_performance() {
        let directory = test_directory("ntfs-performance");
        let db = directory.join("search.sqlite3");
        let volume = std::env::var("LOGCRATE_NTFS_BENCH_VOLUME")
            .ok()
            .and_then(|value| value.chars().next())
            .map(|value| value.to_ascii_uppercase())
            .unwrap_or('C');
        let root = format!("{volume}:\\");
        let mut query_index = SearchIndex::open(&directory.join("tantivy")).unwrap();
        query_index.begin_bulk().unwrap();
        initialize_database(&db).unwrap();
        prepare_bulk_index(&db, true).unwrap();
        let started = std::time::Instant::now();
        let mut records = Vec::new();
        let enumeration = enumerate_mft_via_service(volume, |batch| {
            records.extend(batch);
            Ok(())
        })
        .unwrap();
        let enumerated_at = started.elapsed();
        let mut first_index = None;
        let (_, records) =
            resolve_mft_files_in_batches_retain(&root, records, NTFS_RESOLVE_BATCH, |entries| {
                let files = entries
                    .into_iter()
                    .map(|entry| indexed_mft_entry(&root, entry))
                    .collect::<Vec<_>>();
                query_index.add_batch(&files.iter().map(search_index_entry).collect::<Vec<_>>())?;
                first_index.get_or_insert_with(|| started.elapsed());
                Ok(())
            })
            .unwrap();
        query_index.finish_bulk().unwrap();
        let search_ready_at = started.elapsed();
        let query_started = std::time::Instant::now();
        let (_, total) = query_index.search(&["log".into()], "", 0, 100).unwrap();
        let query_elapsed = query_started.elapsed();
        if std::env::var_os("LOGCRATE_NTFS_BENCH_FAST_PHASE").is_some() {
            eprintln!(
                "NTFS_FAST_PHASE records={} enum_ms={} first_index_ms={} search_ready_ms={} documents={} query_ms={} matches={}",
                enumeration.records,
                enumerated_at.as_millis(),
                first_index.unwrap_or(search_ready_at).as_millis(),
                search_ready_at.as_millis(),
                query_index.num_docs(),
                query_elapsed.as_millis(),
                total,
            );
            drop(query_index);
            let _ = fs::remove_dir_all(directory);
            return;
        }
        let mut connection = open_database(&db).unwrap();
        connection
            .pragma_update(None, "synchronous", "OFF")
            .unwrap();
        connection
            .pragma_update(None, "cache_size", -65_536)
            .unwrap();
        let transaction = connection.transaction().unwrap();
        let (_, records) =
            resolve_mft_files_in_batches_retain(&root, records, NTFS_RESOLVE_BATCH, |entries| {
                let files = entries
                    .into_iter()
                    .map(|entry| indexed_mft_entry(&root, entry))
                    .collect::<Vec<_>>();
                write_file_rows(&transaction, &files, false)
            })
            .unwrap();
        transaction.commit().unwrap();
        let persisted_at = started.elapsed();
        replace_ntfs_nodes(&mut connection, &root, &records).unwrap();
        let nodes_at = started.elapsed();
        if std::env::var_os("LOGCRATE_NTFS_BENCH_NODES_PHASE").is_some() {
            let node_count = connection
                .query_row("SELECT COUNT(*) FROM ntfs_nodes", [], |row| {
                    row.get::<_, u64>(0)
                })
                .unwrap();
            eprintln!(
                "NTFS_NODES_PHASE records={} enum_ms={} first_index_ms={} search_ready_ms={} persisted_ms={} nodes_ms={} nodes={} bytes={}",
                enumeration.records,
                enumerated_at.as_millis(),
                first_index.unwrap_or(search_ready_at).as_millis(),
                search_ready_at.as_millis(),
                persisted_at.as_millis(),
                nodes_at.as_millis(),
                node_count,
                database_size(&db),
            );
            drop(connection);
            let _ = fs::remove_dir_all(directory);
            return;
        }
        drop(connection);
        finish_bulk_index(&db).unwrap();
        let finished_at = started.elapsed();
        eprintln!(
            "NTFS_PERF volume={} records={} batches={} enum_ms={} first_index_ms={} search_ready_ms={} persisted_ms={} nodes_ms={} total_ms={} query_ms={} matches={} sqlite_bytes={} documents={}",
            volume,
            enumeration.records,
            enumeration.batches,
            enumerated_at.as_millis(),
            first_index.unwrap_or(finished_at).as_millis(),
            search_ready_at.as_millis(),
            persisted_at.as_millis(),
            nodes_at.as_millis(),
            finished_at.as_millis(),
            query_elapsed.as_millis(),
            total,
            database_size(&db),
            query_index.num_docs(),
        );
        assert!(enumeration.records > 0);
        assert!(query_elapsed < Duration::from_millis(100));
        drop(query_index);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed LogCrate Index Service and local NTFS C and D volumes"]
    fn windows_multi_volume_application_rebuild_performance() {
        let directory = test_directory("multi-volume-application-performance");
        let manager = FileSearchManager::new(directory.clone());
        {
            let mut config = manager.config.lock().unwrap();
            config.enabled = true;
            config.roots = vec!["C:\\".into(), "D:\\".into()];
            config.exclusions.clear();
        }
        let sink = NoopSearchStatusSink;
        let started = std::time::Instant::now();
        manager.start(sink, true).unwrap();

        let mut scheduled_at = None;
        let mut first_searchable_at = None;
        let mut query_ready_at = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(10 * 60);
        loop {
            let status = manager.status();
            if scheduled_at.is_none()
                && status
                    .providers
                    .iter()
                    .all(|provider| provider.phase != "pending")
            {
                scheduled_at = Some(started.elapsed());
            }
            if first_searchable_at.is_none() && status.indexed_files > 0 {
                first_searchable_at = Some(started.elapsed());
            }
            if query_ready_at.is_none() && status.phase == "ready" {
                query_ready_at = Some(started.elapsed());
            }
            let persistence_complete = query_ready_at.is_some()
                && !manager.persistence_recovery.load(Ordering::Acquire)
                && status
                    .providers
                    .iter()
                    .all(|provider| provider.completed_ms.is_some());
            if persistence_complete {
                let query_started = std::time::Instant::now();
                let c_page = manager.query("notepad.exe", "", 0, 100).unwrap();
                let c_query = query_started.elapsed();
                let query_started = std::time::Instant::now();
                let d_page = manager
                    .query("tauri.dev-static.conf.json", "", 0, 100)
                    .unwrap();
                let d_query = query_started.elapsed();
                let final_status = manager.status();
                eprintln!(
                    "NTFS_APP_PHASE scheduled_ms={} first_searchable_ms={} query_ready_ms={} persisted_ms={} discovered={} searchable={} c_matches={} d_matches={} c_query_ms={} d_query_ms={} providers={:?}",
                    scheduled_at.unwrap_or_default().as_millis(),
                    first_searchable_at.unwrap_or_default().as_millis(),
                    query_ready_at.unwrap_or_default().as_millis(),
                    started.elapsed().as_millis(),
                    final_status.scanned_files,
                    final_status.indexed_files,
                    c_page.items.len(),
                    d_page.items.len(),
                    c_query.as_millis(),
                    d_query.as_millis(),
                    final_status.providers,
                );
                eprintln!(
                    "NTFS_OPERATION_SNAPSHOT {:?}",
                    manager.operation_snapshot_for_report()
                );
                assert!(c_page
                    .items
                    .iter()
                    .any(|item| item.path.starts_with("C:\\")));
                assert!(d_page
                    .items
                    .iter()
                    .any(|item| item.path.starts_with("D:\\")));
                assert_eq!(
                    final_status.indexed_files,
                    final_status
                        .providers
                        .iter()
                        .map(|provider| provider.searchable_files)
                        .sum::<u64>()
                );
                break;
            }
            if status.phase == "error" {
                panic!(
                    "multi-volume application rebuild failed: {:?}",
                    status.error
                );
            }
            assert!(
                std::time::Instant::now() < deadline,
                "multi-volume rebuild timed out"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        manager.pause(&sink);
        drop(manager);
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed LogCrate Index Service and a writable local NTFS D volume"]
    fn windows_directory_change_rebuild_performance() {
        struct TestDirectoryCleanup(Vec<PathBuf>);
        impl Drop for TestDirectoryCleanup {
            fn drop(&mut self) {
                for path in &self.0 {
                    let _ = fs::remove_dir_all(path);
                }
            }
        }

        let volume = 'D';
        let root = "D:\\";
        let unique = format!(
            "LogCrateUsnRecoveryTest-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let created = Path::new(root).join(&unique);
        let renamed = Path::new(root).join(format!("{unique}-renamed"));
        let _cleanup = TestDirectoryCleanup(vec![created.clone(), renamed.clone()]);
        let journal_before = query_usn_via_service(volume).unwrap();
        fs::create_dir(&created).unwrap();
        fs::rename(&created, &renamed).unwrap();
        fs::write(renamed.join("logcrate-usn-recovery-proof.txt"), b"proof").unwrap();
        let journal_after = query_usn_via_service(volume).unwrap();

        let directory = test_directory("directory-change-recovery-performance");
        let db = directory.join("search.sqlite3");
        initialize_database(&db).unwrap();
        let connection = open_database(&db).unwrap();
        let (_, directory_change) =
            collect_usn_changes_until_directory_change(&connection, root, |on_batch| {
                read_usn_via_service(
                    volume,
                    journal_before.next_usn,
                    journal_before.journal_id,
                    journal_after.next_usn,
                    on_batch,
                )
            })
            .unwrap();
        assert!(
            directory_change,
            "the real directory USN change was not classified"
        );
        drop(connection);

        let started = std::time::Instant::now();
        let mut records = Vec::new();
        let enumeration = enumerate_mft_via_service(volume, |batch| {
            records.extend(batch);
            Ok(())
        })
        .unwrap();
        let enumerated_at = started.elapsed();
        let mut query_index = SearchIndex::open(&directory.join("tantivy")).unwrap();
        query_index.begin_bulk().unwrap();
        let mut searchable = 0_u64;
        resolve_mft_files_in_batches(root, records, NTFS_RESOLVE_BATCH, |entries| {
            let files = entries
                .into_iter()
                .map(|entry| indexed_mft_entry(root, entry))
                .collect::<Vec<_>>();
            searchable = searchable.saturating_add(files.len() as u64);
            query_index.add_batch(&files.iter().map(search_index_entry).collect::<Vec<_>>())
        })
        .unwrap();
        query_index.finish_bulk().unwrap();
        let rebuilt_at = started.elapsed();
        let (_, matches) = query_index
            .search(
                &[
                    "logcrate".into(),
                    "usn".into(),
                    "recovery".into(),
                    "proof".into(),
                ],
                "",
                0,
                100,
            )
            .unwrap();
        eprintln!(
            "NTFS_DIRECTORY_RECOVERY volume=D records={} searchable={} enum_ms={} query_ready_ms={} matches={}",
            enumeration.records,
            searchable,
            enumerated_at.as_millis(),
            rebuilt_at.as_millis(),
            matches
        );
        assert!(matches > 0);
        assert!(rebuilt_at <= Duration::from_secs(60));

        drop(query_index);
        fs::remove_file(renamed.join("logcrate-usn-recovery-proof.txt")).unwrap();
        fs::remove_dir(&renamed).unwrap();
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed LogCrate Index Service and a writable local NTFS D volume"]
    fn windows_single_scope_startup_recovery_performance() {
        let directory = test_directory("single-scope-startup-recovery-performance");
        let manager = FileSearchManager::new(directory.clone());
        {
            let mut config = manager.config.lock().unwrap();
            config.enabled = true;
            config.roots = vec!["D:\\".into()];
            config.exclusions.clear();
        }
        let sink = NoopSearchStatusSink;
        manager.start(sink, true).unwrap();
        let build_deadline = std::time::Instant::now() + Duration::from_secs(5 * 60);
        loop {
            let status = manager.status();
            if status.phase == "ready" && !manager.persistence_recovery.load(Ordering::Acquire) {
                break;
            }
            assert!(std::time::Instant::now() < build_deadline);
            std::thread::sleep(Duration::from_millis(100));
        }
        manager.pause(&sink);
        drop(manager);

        let active = directory.join("file-search-orange-gpl-v1");
        fs::remove_dir_all(&active).unwrap();
        let recovered = FileSearchManager::new(directory.clone());
        let started = std::time::Instant::now();
        recovered.resume_or_watch(sink).unwrap();
        let deadline = started + Duration::from_secs(60);
        loop {
            let status = recovered.status();
            if recovered.query_index_ready.load(Ordering::Acquire) && status.indexed_files > 0 {
                let (_, total) = recovered
                    .query_tantivy(&["tauri.dev-static.conf.json".into()], "", 0, 20)
                    .unwrap()
                    .unwrap_or_default();
                eprintln!(
                    "NTFS_STARTUP_RECOVERY scope=D query_ready_ms={} matches={total}",
                    started.elapsed().as_millis()
                );
                assert!(total > 0);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(100));
        }
        recovered.pause(&sink);
        drop(recovered);
        let _ = fs::remove_dir_all(directory);
    }
}
