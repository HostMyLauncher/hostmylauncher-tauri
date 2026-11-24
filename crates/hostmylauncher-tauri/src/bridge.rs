//! Session-scoped provider service, independent of the Tauri runtime.
use crate::contract::*;
use futures_util::FutureExt;
use hostmylauncher::{Client, FileIssueKind, InstanceWatcher, ListOpts, SyncProgress, SyncStage};
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};

pub type EventSink = Arc<dyn Fn(OperationSnapshot) + Send + Sync + 'static>;
pub type CatalogSink = Arc<dyn Fn(CatalogSnapshot) + Send + Sync + 'static>;

/// Where a host-managed installation lives. Never accepted through IPC.
#[derive(Clone, Debug)]
pub struct InstanceLocation {
    pub destination: PathBuf,
    pub manifest_path: PathBuf,
}

/// Why the host refuses to locate an installation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocateError {
    /// The host has no installation for this provider instance (`not_found`).
    NotInstalled,
    /// The host is using the installation, e.g. the game is running (`operation_conflict`).
    Busy,
}

/// The host's answer to "where is this installation?". The launcher keeps its
/// own catalog of installations; the bridge stores none and asks on every start.
///
/// `locate` runs while the bridge holds its admission lock, so it must not call
/// back into the [`Bridge`]. A host that reserves an installation for its own
/// work (launching, uninstalling) should record the reservation where `locate`
/// sees it, release its own locks, and only then check [`Bridge::is_busy`]:
/// either the bridge sees the reservation or the host sees the operation.
pub trait InstanceLocator: Send + Sync + 'static {
    fn locate(&self, launcher_id: &str, instance_id: &str)
        -> Result<InstanceLocation, LocateError>;
}
impl<F> InstanceLocator for F
where
    F: Fn(&str, &str) -> Result<InstanceLocation, LocateError> + Send + Sync + 'static,
{
    fn locate(
        &self,
        launcher_id: &str,
        instance_id: &str,
    ) -> Result<InstanceLocation, LocateError> {
        self(launcher_id, instance_id)
    }
}

#[derive(Clone)]
pub struct Bridge(Arc<Inner>);
struct Inner {
    client: Client,
    catalog: BTreeSet<String>,
    locator: Box<dyn InstanceLocator>,
    state: Mutex<State>,
    sinks: Mutex<Vec<EventSink>>,
    catalog_sinks: Mutex<Vec<CatalogSink>>,
    live: Mutex<LiveCatalog>,
}
/// The single provider catalog kept live over the real-time connection.
#[derive(Default)]
struct LiveCatalog {
    /// Bumped by every watch request, so a slower earlier start cannot win.
    generation: u64,
    task: Option<tokio::task::AbortHandle>,
}
#[derive(Default)]
struct State {
    next_id: u64,
    operations: BTreeMap<String, OperationSnapshot>,
    active: BTreeMap<String, Active>,
}
struct Active {
    launcher_id: String,
    instance_id: String,
    resources: Resources,
}
#[derive(Clone)]
struct Resources {
    root: PathBuf,
    manifest: PathBuf,
}
fn error(code: ErrorCode) -> BridgeError {
    BridgeError {
        code,
        operation_id: None,
        stage: None,
        context: None,
    }
}
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // User callbacks never execute under these locks. Recovering poison avoids
    // stranding a session if an unrelated host thread unwinds.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}
fn code_valid(code: Option<&str>) -> bool {
    code.is_none_or(|code| !code.trim().is_empty())
}
fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}
impl Resources {
    fn conflicts(&self, other: &Self) -> bool {
        overlaps(&self.root, &other.root)
            || overlaps(&self.root, &other.manifest)
            || overlaps(&self.manifest, &other.root)
            || overlaps(&self.manifest, &other.manifest)
    }
}

/// Canonicalize the deepest existing ancestor, then append missing components.
/// Host-configured directory aliases resolve to their real directory. Manifest
/// symlinks are rejected rather than followed as mutable control files.
fn identity(path: &Path, manifest: bool) -> Result<PathBuf, BridgeError> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(error(ErrorCode::InvalidRequest));
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if manifest && (metadata.file_type().is_symlink() || !metadata.is_file()) {
                return Err(error(ErrorCode::InvalidRequest));
            }
            if !manifest
                && !std::fs::metadata(path)
                    .map_err(|_| error(ErrorCode::Filesystem))?
                    .is_dir()
            {
                return Err(error(ErrorCode::InvalidRequest));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(error(ErrorCode::Filesystem)),
    }
    let mut ancestor = path.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(&ancestor) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    ancestor
                        .file_name()
                        .ok_or_else(|| error(ErrorCode::InvalidRequest))?
                        .to_owned(),
                );
                if !ancestor.pop() {
                    return Err(error(ErrorCode::InvalidRequest));
                }
            }
            Err(_) => return Err(error(ErrorCode::Filesystem)),
        }
    }
    let mut result = std::fs::canonicalize(&ancestor).map_err(|_| error(ErrorCode::Filesystem))?;
    if !missing.is_empty() && !result.is_dir() {
        return Err(error(ErrorCode::InvalidRequest));
    }
    for part in missing.into_iter().rev() {
        result.push(part);
    }
    Ok(result)
}
fn resources(location: &InstanceLocation) -> Result<Resources, BridgeError> {
    let root = identity(&location.destination, false)?;
    let manifest = identity(&location.manifest_path, true)?;
    // A manifest may live inside its own destination, but cannot be the root
    // itself or an ancestor that would need to be both a file and a directory.
    if root.starts_with(&manifest) {
        return Err(error(ErrorCode::InvalidRequest));
    }
    Ok(Resources { root, manifest })
}

impl Bridge {
    pub fn new(
        client: Client,
        catalog: Vec<String>,
        locator: impl InstanceLocator,
        sink: EventSink,
    ) -> Result<Self, BridgeError> {
        if catalog.is_empty() || catalog.iter().any(|id| !identifier(id)) {
            return Err(error(ErrorCode::InvalidRequest));
        }
        Ok(Self(Arc::new(Inner {
            client,
            catalog: catalog.into_iter().collect(),
            locator: Box::new(locator),
            state: Mutex::new(State::default()),
            sinks: Mutex::new(vec![sink]),
            catalog_sinks: Mutex::new(Vec::new()),
            live: Mutex::new(LiveCatalog::default()),
        })))
    }

    /// Whether an accepted operation on this provider instance has not finished.
    /// Hosts check it before launching, uninstalling or exiting.
    pub fn is_busy(&self, launcher_id: &str, instance_id: &str) -> bool {
        lock(&self.0.state)
            .active
            .values()
            .any(|active| active.launcher_id == launcher_id && active.instance_id == instance_id)
    }

    /// Whether any accepted operation has not finished.
    pub fn has_active_operations(&self) -> bool {
        !lock(&self.0.state).active.is_empty()
    }

    fn locate(&self, launcher_id: &str, instance_id: &str) -> Result<Resources, BridgeError> {
        let located = catch_unwind(AssertUnwindSafe(|| {
            self.0.locator.locate(launcher_id, instance_id)
        }))
        .map_err(|_| error(ErrorCode::Internal))?;
        match located {
            Ok(location) => resources(&location),
            Err(LocateError::NotInstalled) => Err(error(ErrorCode::NotFound)),
            Err(LocateError::Busy) => Err(error(ErrorCode::OperationConflict)),
        }
    }

    /// Attach a host event emitter; called once by plugin setup. Like the initial
    /// sink, it receives public snapshots outside all service locks.
    pub fn add_event_sink(&self, sink: EventSink) {
        lock(&self.0.sinks).push(sink);
    }

    /// Attach a host emitter for live catalog snapshots; called once by plugin setup.
    pub fn add_catalog_sink(&self, sink: CatalogSink) {
        lock(&self.0.catalog_sinks).push(sink);
    }

    fn emit_catalog(&self, snapshot: CatalogSnapshot) {
        let sinks = lock(&self.0.catalog_sinks).clone();
        for sink in sinks {
            // Best-effort like operation events; a failing sink cannot stop the watch.
            let _ = catch_unwind(AssertUnwindSafe(|| sink(snapshot.clone())));
        }
    }

    fn emit(&self, snapshot: OperationSnapshot) -> Result<(), BridgeError> {
        let sinks = lock(&self.0.sinks).clone();
        let mut failed = false;
        for sink in sinks {
            if catch_unwind(AssertUnwindSafe(|| sink(snapshot.clone()))).is_err() {
                failed = true;
            }
        }
        if failed {
            Err(error(ErrorCode::Internal))
        } else {
            Ok(())
        }
    }
    fn catalog(&self, launcher: &str) -> Result<(), BridgeError> {
        if !identifier(launcher) || !self.0.catalog.contains(launcher) {
            Err(error(ErrorCode::InvalidRequest))
        } else {
            Ok(())
        }
    }
    fn instance_input(&self, launcher: &str, instance: &str) -> Result<(), BridgeError> {
        self.catalog(launcher)?;
        if !identifier(instance) {
            return Err(error(ErrorCode::InvalidRequest));
        }
        Ok(())
    }
    fn project_instance(
        &self,
        instance: hostmylauncher::Instance,
    ) -> Result<InstanceDto, BridgeError> {
        // Even a malicious/misconfigured provider must not expand the catalog.
        if instance
            .launcher_id
            .as_ref()
            .is_some_and(|id| !self.0.catalog.contains(id))
        {
            return Err(error(ErrorCode::InvalidRequest));
        }
        Ok(InstanceDto {
            id: instance.id,
            launcher_id: instance.launcher_id,
            slug: instance.slug,
            title: instance.title,
            minecraft_version: instance.minecraft_version,
            server: instance.server,
            loader: instance.loader.map(|loader| LoaderDto {
                kind: loader.kind,
                build: loader.build,
                enabled: loader.enabled,
            }),
            java_version: instance.java_version,
            suggested_ram: instance.suggested_ram,
            icon: instance.icon,
            landscape: instance.landscape,
            poster: instance.poster,
            animation: instance.animation,
            locked: instance.locked,
            no_premium: instance.users.is_some_and(|users| users.no_premium),
            downloads: instance.downloads,
        })
    }
    pub async fn list_instances(
        &self,
        request: ListInstancesRequest,
    ) -> Result<Vec<InstanceDto>, BridgeError> {
        self.catalog(&request.launcher_id)?;
        let instances = self
            .0
            .client
            .list_instances(
                &request.launcher_id,
                &ListOpts {
                    limit: request.limit,
                    offset: request.offset,
                    ..Default::default()
                },
            )
            .await
            .map_err(map_error)?;
        instances
            .into_iter()
            .map(|instance| self.project_instance(instance))
            .collect()
    }
    /// Keeps one provider's public catalog live and returns its current list.
    /// Later changes arrive as [`CatalogSnapshot`]s through the catalog sinks.
    /// Watching another provider, or `None`, closes the previous connection.
    pub async fn watch_catalog(
        &self,
        request: WatchCatalogRequest,
    ) -> Result<Vec<InstanceDto>, BridgeError> {
        if let Some(launcher_id) = &request.launcher_id {
            self.catalog(launcher_id)?;
        }
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|_| error(ErrorCode::Internal))?;
        let generation = {
            let mut live = lock(&self.0.live);
            live.generation += 1;
            if let Some(task) = live.task.take() {
                task.abort();
            }
            live.generation
        };
        let Some(launcher_id) = request.launcher_id else {
            return Ok(Vec::new());
        };
        let mut watcher = InstanceWatcher::start(self.0.client.clone(), &launcher_id)
            .await
            .map_err(map_error)?;
        let instances = self.project_catalog(watcher.current());

        let mut live = lock(&self.0.live);
        if live.generation != generation {
            // A newer request took over; dropping the watcher closes its socket.
            return Ok(instances);
        }
        let bridge = Arc::downgrade(&self.0);
        let mut last = instances.clone();
        live.task = Some(
            runtime
                .spawn(async move {
                    while watcher.changed().await {
                        let Some(inner) = Weak::upgrade(&bridge) else {
                            break;
                        };
                        let bridge = Bridge(inner);
                        let next = bridge.project_catalog(watcher.current());
                        if next != last {
                            last = next.clone();
                            bridge.emit_catalog(CatalogSnapshot {
                                launcher_id: launcher_id.clone(),
                                instances: next,
                            });
                        }
                    }
                })
                .abort_handle(),
        );
        Ok(instances)
    }
    /// Live lists skip entries outside the catalog instead of failing the watch.
    fn project_catalog(&self, instances: Vec<hostmylauncher::Instance>) -> Vec<InstanceDto> {
        instances
            .into_iter()
            .filter_map(|instance| self.project_instance(instance).ok())
            .collect()
    }
    pub async fn get_instance(
        &self,
        request: GetInstanceRequest,
    ) -> Result<InstanceDto, BridgeError> {
        self.instance_input(&request.launcher_id, &request.instance_id)?;
        self.project_instance(
            self.0
                .client
                .get_instance(&request.launcher_id, &request.instance_id)
                .await
                .map_err(map_error)?,
        )
    }
    pub async fn resolve_by_code(
        &self,
        request: ResolveByCodeRequest,
    ) -> Result<InstanceDto, BridgeError> {
        if !(1..=20).contains(&request.launcher_ids.len()) || !code_valid(Some(&request.code)) {
            return Err(error(ErrorCode::InvalidRequest));
        }
        for id in &request.launcher_ids {
            self.catalog(id)?;
        }
        let ids: Vec<_> = request.launcher_ids.iter().map(String::as_str).collect();
        let instance = self
            .0
            .client
            .resolve_by_code(&ids, &request.code)
            .await
            .map_err(map_error)?;
        if !instance
            .launcher_id
            .as_ref()
            .is_some_and(|id| request.launcher_ids.contains(id))
        {
            return Err(error(ErrorCode::InvalidRequest));
        }
        self.project_instance(instance)
    }
    pub async fn get_instance_files(
        &self,
        request: GetInstanceFilesRequest,
    ) -> Result<Vec<FileDescriptorDto>, BridgeError> {
        self.instance_input(&request.launcher_id, &request.instance_id)?;
        if !code_valid(request.code.as_deref()) {
            return Err(error(ErrorCode::InvalidRequest));
        }
        let files = self
            .0
            .client
            .get_instance_files(
                &request.launcher_id,
                &request.instance_id,
                request.code.as_deref(),
            )
            .await
            .map_err(map_error)?;
        Ok(files
            .into_iter()
            .map(|file| FileDescriptorDto {
                path: file.path,
                name: file.name,
                size: file.size,
                hashes: file.hashes.map(|hashes| HashesDto {
                    sha1: hashes.sha1,
                    sha512: hashes.sha512,
                }),
            })
            .collect())
    }
    pub async fn get_operation(
        &self,
        request: GetOperationRequest,
    ) -> Result<OperationSnapshot, BridgeError> {
        lock(&self.0.state)
            .operations
            .get(&request.operation_id)
            .cloned()
            .ok_or_else(|| error(ErrorCode::NotFound))
    }
    pub async fn start_operation(
        &self,
        request: StartOperationRequest,
    ) -> Result<OperationSnapshot, BridgeError> {
        self.catalog(&request.launcher_id)?;
        if !identifier(&request.instance_id) || !code_valid(request.code.as_deref()) {
            return Err(error(ErrorCode::InvalidRequest));
        }
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|_| error(ErrorCode::Internal))?;
        let (accepted, resources) = {
            let mut state = lock(&self.0.state);
            // Ask the host under the admission lock, so a host reservation and an
            // accepted operation cannot both miss each other (see InstanceLocator).
            let resources = self.locate(&request.launcher_id, &request.instance_id)?;
            if state
                .active
                .values()
                .any(|active| resources.conflicts(&active.resources))
            {
                return Err(error(ErrorCode::OperationConflict));
            }
            let next = state
                .next_id
                .checked_add(1)
                .ok_or_else(|| error(ErrorCode::Internal))?;
            let id = format!("operation-{next}");
            let accepted = OperationSnapshot {
                operation_id: id.clone(),
                launcher_id: request.launcher_id.clone(),
                instance_id: request.instance_id.clone(),
                kind: request.kind,
                status: OperationStatus::Queued,
                sequence: 0,
                progress: None,
                result: None,
                error: None,
            };
            state.next_id = next;
            state.active.insert(
                id.clone(),
                Active {
                    launcher_id: request.launcher_id.clone(),
                    instance_id: request.instance_id.clone(),
                    resources: resources.clone(),
                },
            );
            state.operations.insert(id, accepted.clone());
            (accepted, resources)
        };
        // An accepted response is always queued, even if its task finishes before
        // this async method returns. A sink failure becomes a stored task failure.
        let queued_emit = self.emit(accepted.clone());
        let bridge = self.clone();
        let id = accepted.operation_id.clone();
        runtime.spawn(async move {
            let outcome = AssertUnwindSafe(async {
                bridge.update(
                    &id,
                    Some(OperationStatus::Running),
                    Some(indeterminate(ProgressStage::FetchingFiles)),
                )?;
                queued_emit?;
                bridge.execute(&id, &resources, request).await
            })
            .catch_unwind()
            .await;
            let outcome = outcome.unwrap_or_else(|_| Err(error(ErrorCode::Internal)));
            bridge.finish(&id, outcome);
        });
        Ok(accepted)
    }
    fn update(
        &self,
        id: &str,
        status: Option<OperationStatus>,
        progress: Option<OperationProgress>,
    ) -> Result<(), BridgeError> {
        let snapshot = {
            let mut state = lock(&self.0.state);
            let Some(snapshot) = state.operations.get_mut(id) else {
                return Ok(());
            };
            if matches!(
                snapshot.status,
                OperationStatus::Succeeded | OperationStatus::Failed
            ) {
                return Ok(());
            }
            if let Some(status) = status {
                snapshot.status = status;
            }
            snapshot.progress = progress;
            snapshot.sequence += 1;
            snapshot.clone()
        };
        self.emit(snapshot)
    }
    fn finish(&self, id: &str, outcome: Result<OperationResult, BridgeError>) {
        let snapshot = {
            let mut state = lock(&self.0.state);
            let Some(snapshot) = state.operations.get_mut(id) else {
                return;
            };
            if matches!(
                snapshot.status,
                OperationStatus::Succeeded | OperationStatus::Failed
            ) {
                return;
            }
            snapshot.sequence += 1;
            match outcome {
                Ok(result) => {
                    snapshot.status = OperationStatus::Succeeded;
                    snapshot.result = Some(result);
                    snapshot.error = None;
                }
                Err(mut error) => {
                    error.operation_id = Some(id.to_owned());
                    error.stage = snapshot.progress.as_ref().map(|progress| progress.stage);
                    snapshot.status = OperationStatus::Failed;
                    snapshot.result = None;
                    snapshot.error = Some(error);
                }
            }
            let snapshot = snapshot.clone();
            state.active.remove(id);
            snapshot
        };
        // Terminal emission is best-effort and cannot revive or change the outcome.
        let _ = self.emit(snapshot);
    }
    async fn execute(
        &self,
        id: &str,
        resources: &Resources,
        request: StartOperationRequest,
    ) -> Result<OperationResult, BridgeError> {
        let files = self
            .0
            .client
            .get_instance_files(
                &request.launcher_id,
                &request.instance_id,
                request.code.as_deref(),
            )
            .await
            .map_err(map_error)?;
        let callback_bridge = self.clone();
        let callback_id = id.to_owned();
        let callback = move |progress: SyncProgress| {
            // Paths from callbacks are deliberately not retained in public state.
            let mapped = progress_dto(progress);
            if callback_bridge
                .update(&callback_id, None, Some(mapped))
                .is_err()
            {
                // Sanitized unwind is caught around the entire operation future.
                std::panic::panic_any(());
            }
        };
        match request.kind {
            OperationKind::Sync => {
                let report = self
                    .0
                    .client
                    .sync_instance_files(&files, &resources.root, &resources.manifest, callback)
                    .await
                    .map_err(map_error)?;
                Ok(OperationResult::Sync {
                    downloaded: report.downloaded as u64,
                    removed: report.removed as u64,
                })
            }
            OperationKind::Verify => {
                self.update(id, None, Some(indeterminate(ProgressStage::Verify)))?;
                let report = self
                    .0
                    .client
                    .verify_instance_files(&files, &resources.root)
                    .await
                    .map_err(map_error)?;
                let report = verification_dto(report);
                Ok(OperationResult::Verify {
                    checked: report.checked,
                    issues: report.issues,
                })
            }
            OperationKind::Repair => {
                let report = self
                    .0
                    .client
                    .repair_instance_files(&files, &resources.root, &resources.manifest, callback)
                    .await
                    .map_err(map_error)?;
                Ok(OperationResult::Repair {
                    verification: verification_dto(report.verification),
                    repaired: report.repaired as u64,
                    downloaded: report.downloaded as u64,
                })
            }
        }
    }
}
fn indeterminate(stage: ProgressStage) -> OperationProgress {
    OperationProgress {
        stage,
        current: None,
        total: None,
        unit: None,
    }
}
fn progress_dto(progress: SyncProgress) -> OperationProgress {
    let (stage, unit) = match progress.stage {
        SyncStage::Download => (ProgressStage::Download, ProgressUnit::Bytes),
        SyncStage::Extract => (ProgressStage::Extract, ProgressUnit::Archives),
        SyncStage::Install => (ProgressStage::Install, ProgressUnit::Entries),
        SyncStage::Remove => (ProgressStage::Remove, ProgressUnit::Entries),
    };
    OperationProgress {
        stage,
        current: Some(progress.current),
        total: (progress.total != 0).then_some(progress.total),
        unit: Some(unit),
    }
}
fn verification_dto(report: hostmylauncher::VerificationReport) -> VerificationReport {
    VerificationReport {
        checked: report.checked as u64,
        issues: report
            .issues
            .into_iter()
            .map(|issue| VerificationIssue {
                path: issue.path,
                kind: match issue.kind {
                    FileIssueKind::Missing => VerificationIssueKind::Missing,
                    FileIssueKind::Different => VerificationIssueKind::Different,
                },
            })
            .collect(),
    }
}
fn map_error(source: hostmylauncher::Error) -> BridgeError {
    use hostmylauncher::Error;
    match source {
        Error::NotFound => error(ErrorCode::NotFound),
        Error::Unauthorized => error(ErrorCode::Unauthorized),
        Error::Api { status, .. } => BridgeError {
            context: Some(ErrorContext {
                http_status: status,
            }),
            ..error(ErrorCode::Api)
        },
        Error::Http(_) | Error::Ws(_) => error(ErrorCode::Network),
        Error::Io(_) => error(ErrorCode::Filesystem),
        Error::Decode(_) => error(ErrorCode::Decode),
        Error::Url(_) | Error::Other(_) => error(ErrorCode::Internal),
    }
}
