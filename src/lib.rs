use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc::{self, RecvTimeoutError, Sender},
    time::{Duration, Instant, UNIX_EPOCH},
};

use notify::{
    Config, Event, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher,
    event::{EventKind, ModifyKind},
};
use serde::Serialize;

pub const SCHEMA_VERSION: u8 = 1;
pub const DEFAULT_MAX_ENTRIES: usize = 100_000;
pub const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendMode {
    Native,
    Poll,
}

impl BackendMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Poll => "poll",
        }
    }
}

impl FromStr for BackendMode {
    type Err = FsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "native" => Ok(Self::Native),
            "poll" => Ok(Self::Poll),
            _ => Err(FsError::new(format!(
                "backend must be `native` or `poll`, got `{value}`"
            ))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScanLimits {
    pub max_entries: usize,
    pub max_bytes: u64,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WatchOptions {
    pub backend: BackendMode,
    pub duration: Option<Duration>,
    pub poll_interval: Duration,
    pub limits: ScanLimits,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            backend: BackendMode::Native,
            duration: None,
            poll_interval: DEFAULT_POLL_INTERVAL,
            limits: ScanLimits::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsError {
    detail: String,
}

impl FsError {
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for FsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for FsError {}

impl From<std::io::Error> for FsError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<notify::Error> for FsError {
    fn from(error: notify::Error) -> Self {
        Self::new(error.to_string())
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FilesystemInfo {
    pub kind: String,
    pub classification: String,
    pub likely_network_or_virtual: bool,
    pub mount_point: Option<String>,
    pub capabilities: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct BackendInfo {
    pub requested: String,
    pub selected: String,
    pub guarantee: String,
    pub capabilities: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SnapshotSummary {
    pub entries: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: String,
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct InspectReport {
    pub schema_version: u8,
    pub tool: String,
    pub path: String,
    pub canonical_path: String,
    pub filesystem: FilesystemInfo,
    pub backend: BackendInfo,
    pub snapshot: SnapshotSummary,
    pub diagnostics: Vec<Diagnostic>,
}

impl InspectReport {
    pub fn exit_code(&self) -> i32 {
        0
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CanonicalEvent {
    pub record_type: String,
    pub sequence: u64,
    pub kind: String,
    pub path: String,
    pub old_path: Option<String>,
    pub source: String,
    pub confidence: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct VerifyReport {
    pub schema_version: u8,
    pub tool: String,
    pub path: String,
    pub filesystem: FilesystemInfo,
    pub backend: BackendInfo,
    pub guarantee: String,
    pub status: String,
    pub duration_ms: u128,
    pub before: SnapshotSummary,
    pub after: SnapshotSummary,
    pub native_events: usize,
    pub canonical_events: Vec<CanonicalEvent>,
    pub diagnostics: Vec<Diagnostic>,
}

impl VerifyReport {
    pub fn exit_code(&self) -> i32 {
        if self.status == "ok" { 0 } else { 3 }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WatchSummary {
    pub schema_version: u8,
    pub tool: String,
    pub path: String,
    pub backend: BackendInfo,
    pub status: String,
    pub snapshots: usize,
    pub native_events: usize,
    pub events_emitted: usize,
    pub diagnostics: Vec<Diagnostic>,
}

impl WatchSummary {
    pub fn exit_code(&self) -> i32 {
        if self.status == "ok" { 0 } else { 3 }
    }
}

type Snapshot = BTreeMap<String, Fingerprint>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Fingerprint {
    kind: EntryKind,
    bytes: u64,
    modified_ns: Option<i128>,
    permissions: u32,
    link_target: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Change {
    kind: String,
    path: String,
    old_path: Option<String>,
}

#[derive(Clone, Debug)]
struct NativeRecord {
    kind: String,
    path: String,
    old_path: Option<String>,
    rescan_required: bool,
}

enum RuntimeWatcher {
    Native { _watcher: RecommendedWatcher },
    Poll { _watcher: PollWatcher },
}

struct WatcherState {
    _runtime: RuntimeWatcher,
    selected: String,
    fallback_reason: Option<String>,
}

pub fn parse_duration(value: &str) -> Result<Duration, FsError> {
    let split = value
        .find(|character: char| !character.is_ascii_digit())
        .ok_or_else(|| FsError::new("duration requires a unit: ms, s, or m"))?;
    let (number, unit) = value.split_at(split);
    if number.is_empty() || unit.is_empty() {
        return Err(FsError::new(
            "duration requires digits followed by ms, s, or m",
        ));
    }
    let number = number
        .parse::<u64>()
        .map_err(|error| FsError::new(format!("invalid duration: {error}")))?;
    let multiplier = match unit {
        "ms" => 1_000_000,
        "s" => 1_000_000_000,
        "m" => 60 * 1_000_000_000,
        _ => return Err(FsError::new("duration unit must be ms, s, or m")),
    };
    let nanos = number
        .checked_mul(multiplier)
        .ok_or_else(|| FsError::new("duration is too large"))?;
    Ok(Duration::from_nanos(nanos))
}

pub fn inspect(path: &Path, limits: &ScanLimits) -> Result<InspectReport, FsError> {
    let root = validate_path(path)?;
    let filesystem = filesystem_info(&root);
    let snapshot = scan_snapshot(&root, limits)?;
    let mut diagnostics = Vec::new();
    if filesystem.kind == "unknown" {
        diagnostics.push(Diagnostic {
            severity: "warning".to_owned(),
            code: "filesystem-type-unknown".to_owned(),
            detail: "filesystem type could not be identified from local mount metadata".to_owned(),
        });
    }
    Ok(InspectReport {
        schema_version: SCHEMA_VERSION,
        tool: "fsproof".to_owned(),
        path: path.to_string_lossy().into_owned(),
        canonical_path: redact_absolute_path(&root.to_string_lossy()),
        filesystem: filesystem.clone(),
        backend: backend_info(
            BackendMode::Native,
            native_backend_name(),
            &filesystem,
            None,
        ),
        snapshot: snapshot_summary(&snapshot),
        diagnostics,
    })
}

pub fn verify(
    path: &Path,
    duration: Duration,
    options: &WatchOptions,
) -> Result<VerifyReport, FsError> {
    if duration.is_zero() {
        return Err(FsError::new("verify duration must be greater than zero"));
    }
    let root = validate_path(path)?;
    let filesystem = filesystem_info(&root);
    let before = scan_snapshot(&root, &options.limits)?;
    let (sender, receiver) = mpsc::channel();
    let watcher = start_watcher(&root, options.backend, options.poll_interval, &sender)?;
    let mut diagnostics = watcher_diagnostics(&watcher);
    let selected_backend = watcher.selected.clone();
    let collect_native_events = selected_backend != "poll";
    let fallback_reason = watcher.fallback_reason.clone();
    let started = Instant::now();
    let deadline = started + duration;
    let mut native_records = Vec::new();
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait = remaining.min(Duration::from_millis(100));
        match receiver.recv_timeout(wait) {
            Ok(Ok(event)) if collect_native_events => {
                native_records.extend(native_records_from_event(&root, &event));
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => diagnostics.push(Diagnostic {
                severity: "degraded".to_owned(),
                code: "native-event-error".to_owned(),
                detail: error.to_string(),
            }),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                diagnostics.push(Diagnostic {
                    severity: "degraded".to_owned(),
                    code: "watcher-disconnected".to_owned(),
                    detail: "filesystem watcher channel disconnected before verification ended"
                        .to_owned(),
                });
                break;
            }
        }
    }
    drop(watcher);
    let after = scan_snapshot(&root, &options.limits)?;
    let (canonical_events, reconciliation_diagnostics) =
        reconcile(&before, &after, &native_records);
    diagnostics.extend(reconciliation_diagnostics);
    sort_diagnostics(&mut diagnostics);
    let status = if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "degraded" || diagnostic.severity == "error")
    {
        "degraded"
    } else {
        "ok"
    };
    Ok(VerifyReport {
        schema_version: SCHEMA_VERSION,
        tool: "fsproof".to_owned(),
        path: path.to_string_lossy().into_owned(),
        filesystem: filesystem.clone(),
        backend: backend_info(
            options.backend,
            &selected_backend,
            &filesystem,
            fallback_reason.as_deref(),
        ),
        guarantee: "best-effort; native notifications do not prove completeness".to_owned(),
        status: status.to_owned(),
        duration_ms: started.elapsed().as_millis(),
        before: snapshot_summary(&before),
        after: snapshot_summary(&after),
        native_events: native_records.len(),
        canonical_events,
        diagnostics,
    })
}

pub fn watch<W: Write>(
    path: &Path,
    options: &WatchOptions,
    mut writer: W,
) -> Result<WatchSummary, FsError> {
    if options.duration.is_some_and(|duration| duration.is_zero()) {
        return Err(FsError::new("watch duration must be greater than zero"));
    }
    let root = validate_path(path)?;
    let filesystem = filesystem_info(&root);
    let mut baseline = scan_snapshot(&root, &options.limits)?;
    let (sender, receiver) = mpsc::channel();
    let watcher = start_watcher(&root, options.backend, options.poll_interval, &sender)?;
    let backend = backend_info(
        options.backend,
        &watcher.selected,
        &filesystem,
        watcher.fallback_reason.as_deref(),
    );
    let collect_native_events = watcher.selected != "poll";
    let mut diagnostics = watcher_diagnostics(&watcher);
    let header = WatchHeader {
        record_type: "ready".to_owned(),
        schema_version: SCHEMA_VERSION,
        tool: "fsproof".to_owned(),
        path: path.to_string_lossy().into_owned(),
        backend: backend.clone(),
        guarantee: "best-effort; native notifications do not prove completeness".to_owned(),
    };
    write_json_line(&mut writer, &header)?;
    let started = Instant::now();
    let mut next_snapshot = started + options.poll_interval;
    let deadline = options.duration.map(|duration| started + duration);
    let mut snapshots = 1usize;
    let mut native_records = Vec::new();
    let mut native_event_count = 0usize;
    let mut events_emitted = 0usize;
    loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        let until_deadline = deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(100));
        let until_snapshot = next_snapshot.saturating_duration_since(Instant::now());
        let wait = until_deadline
            .min(until_snapshot)
            .min(Duration::from_millis(100));
        match receiver.recv_timeout(wait) {
            Ok(Ok(event)) if collect_native_events => {
                let records = native_records_from_event(&root, &event);
                native_event_count += records.len();
                native_records.extend(records);
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => diagnostics.push(Diagnostic {
                severity: "degraded".to_owned(),
                code: "native-event-error".to_owned(),
                detail: error.to_string(),
            }),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                diagnostics.push(Diagnostic {
                    severity: "degraded".to_owned(),
                    code: "watcher-disconnected".to_owned(),
                    detail: "filesystem watcher channel disconnected before watch ended".to_owned(),
                });
                break;
            }
        }
        if Instant::now() >= next_snapshot
            || deadline.is_some_and(|deadline| Instant::now() >= deadline)
        {
            let current = scan_snapshot(&root, &options.limits)?;
            let (events, reconciliation_diagnostics) =
                reconcile(&baseline, &current, &native_records);
            for event in &events {
                write_json_line(&mut writer, event)?;
            }
            events_emitted += events.len();
            diagnostics.extend(reconciliation_diagnostics);
            baseline = current;
            native_records.clear();
            snapshots += 1;
            next_snapshot = Instant::now() + options.poll_interval;
        }
    }
    drop(watcher);
    writer.flush()?;
    sort_diagnostics(&mut diagnostics);
    let status = if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "degraded" || diagnostic.severity == "error")
    {
        "degraded"
    } else {
        "ok"
    };
    Ok(WatchSummary {
        schema_version: SCHEMA_VERSION,
        tool: "fsproof".to_owned(),
        path: path.to_string_lossy().into_owned(),
        backend,
        status: status.to_owned(),
        snapshots,
        native_events: native_event_count,
        events_emitted,
        diagnostics,
    })
}

pub fn render_json<T: Serialize>(value: &T) -> serde_json::Result<String> {
    serde_json::to_string_pretty(value)
}

pub fn render_inspect_text(report: &InspectReport) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "fsproof schema={}", report.schema_version);
    let _ = writeln!(output, "path: {}", report.path);
    let _ = writeln!(output, "canonical_path: {}", report.canonical_path);
    let _ = writeln!(output, "filesystem: {}", report.filesystem.kind);
    let _ = writeln!(
        output,
        "classification: {}",
        report.filesystem.classification
    );
    let _ = writeln!(output, "backend: {}", report.backend.selected);
    let _ = writeln!(output, "guarantee: {}", report.backend.guarantee);
    let _ = writeln!(
        output,
        "snapshot: entries={} bytes={}",
        report.snapshot.entries, report.snapshot.bytes
    );
    append_diagnostics(&mut output, &report.diagnostics);
    output
}

pub fn render_verify_text(report: &VerifyReport) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "fsproof schema={}", report.schema_version);
    let _ = writeln!(output, "status: {}", report.status);
    let _ = writeln!(output, "path: {}", report.path);
    let _ = writeln!(output, "filesystem: {}", report.filesystem.kind);
    let _ = writeln!(output, "backend: {}", report.backend.selected);
    let _ = writeln!(output, "guarantee: {}", report.guarantee);
    let _ = writeln!(output, "duration_ms: {}", report.duration_ms);
    let _ = writeln!(
        output,
        "snapshots: before_entries={} after_entries={}",
        report.before.entries, report.after.entries
    );
    let _ = writeln!(output, "native_events: {}", report.native_events);
    let _ = writeln!(
        output,
        "canonical_events: {}",
        report.canonical_events.len()
    );
    for event in &report.canonical_events {
        let _ = writeln!(
            output,
            "- {} path={} old_path={} source={} confidence={}",
            event.kind,
            event.path,
            event.old_path.as_deref().unwrap_or("none"),
            event.source,
            event.confidence
        );
    }
    append_diagnostics(&mut output, &report.diagnostics);
    output
}

#[derive(Serialize)]
struct WatchHeader {
    record_type: String,
    schema_version: u8,
    tool: String,
    path: String,
    backend: BackendInfo,
    guarantee: String,
}

fn write_json_line<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), FsError> {
    let line = serde_json::to_vec(value).map_err(|error| FsError::new(error.to_string()))?;
    writer.write_all(&line)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn validate_path(path: &Path) -> Result<PathBuf, FsError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| FsError::new(format!("cannot inspect `{}`: {error}", path.display())))?;
    if !metadata.is_dir() && !metadata.is_file() {
        return Err(FsError::new(format!(
            "watch path `{}` must be a file or directory",
            path.display()
        )));
    }
    path.canonicalize()
        .map_err(|error| FsError::new(format!("cannot canonicalize `{}`: {error}", path.display())))
}

fn scan_snapshot(root: &Path, limits: &ScanLimits) -> Result<Snapshot, FsError> {
    if limits.max_entries == 0 {
        return Err(FsError::new("max entries must be greater than zero"));
    }
    let mut snapshot = BTreeMap::new();
    let mut bytes = 0u64;
    visit_snapshot(root, root, limits, &mut snapshot, &mut bytes)?;
    Ok(snapshot)
}

fn visit_snapshot(
    root: &Path,
    path: &Path,
    limits: &ScanLimits,
    snapshot: &mut Snapshot,
    bytes: &mut u64,
) -> Result<(), FsError> {
    if snapshot.len() >= limits.max_entries {
        return Err(FsError::new(format!(
            "snapshot exceeded max entries limit of {}",
            limits.max_entries
        )));
    }
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    };
    let entry_bytes = if kind == EntryKind::File {
        metadata.len()
    } else {
        0
    };
    *bytes = bytes
        .checked_add(entry_bytes)
        .ok_or_else(|| FsError::new("snapshot byte count overflowed"))?;
    if *bytes > limits.max_bytes {
        return Err(FsError::new(format!(
            "snapshot exceeded max bytes limit of {}",
            limits.max_bytes
        )));
    }
    let link_target = if kind == EntryKind::Symlink {
        Some(fs::read_link(path)?.to_string_lossy().into_owned())
    } else {
        None
    };
    snapshot.insert(
        relative_path(root, path),
        Fingerprint {
            kind: kind.clone(),
            bytes: entry_bytes,
            modified_ns: modified_ns(&metadata),
            permissions: permissions_mode(&metadata),
            link_target,
        },
    );
    if kind == EntryKind::Directory {
        let mut children = fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        children.sort();
        for child in children {
            visit_snapshot(root, &child, limits, snapshot, bytes)?;
        }
    }
    Ok(())
}

fn snapshot_summary(snapshot: &Snapshot) -> SnapshotSummary {
    SnapshotSummary {
        entries: snapshot.len(),
        bytes: snapshot.values().map(|entry| entry.bytes).sum(),
    }
}

fn snapshot_diff(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut removed = before
        .iter()
        .filter(|(path, _)| !after.contains_key(*path))
        .map(|(path, fingerprint)| (path.clone(), fingerprint.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut added = after
        .iter()
        .filter(|(path, _)| !before.contains_key(*path))
        .map(|(path, fingerprint)| (path.clone(), fingerprint.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    let mut removed_by_fingerprint = BTreeMap::<Fingerprint, Vec<String>>::new();
    let mut added_by_fingerprint = BTreeMap::<Fingerprint, Vec<String>>::new();
    for (path, fingerprint) in &removed {
        removed_by_fingerprint
            .entry(fingerprint.clone())
            .or_default()
            .push(path.clone());
    }
    for (path, fingerprint) in &added {
        added_by_fingerprint
            .entry(fingerprint.clone())
            .or_default()
            .push(path.clone());
    }
    for (fingerprint, old_paths) in removed_by_fingerprint {
        let Some(new_paths) = added_by_fingerprint.get(&fingerprint) else {
            continue;
        };
        if old_paths.len() == 1 && new_paths.len() == 1 {
            let old_path = old_paths[0].clone();
            let new_path = new_paths[0].clone();
            removed.remove(&old_path);
            added.remove(&new_path);
            changes.push(Change {
                kind: "Renamed".to_owned(),
                path: new_path,
                old_path: Some(old_path),
            });
        }
    }
    changes.extend(removed.into_keys().map(|path| Change {
        kind: "Removed".to_owned(),
        path,
        old_path: None,
    }));
    changes.extend(added.into_keys().map(|path| Change {
        kind: "Created".to_owned(),
        path,
        old_path: None,
    }));
    for (path, old_fingerprint) in before {
        if let Some(new_fingerprint) = after.get(path)
            && old_fingerprint != new_fingerprint
        {
            changes.push(Change {
                kind: "Modified".to_owned(),
                path: path.clone(),
                old_path: None,
            });
        }
    }
    changes.sort_by(|left, right| {
        (&left.kind, &left.path, &left.old_path).cmp(&(&right.kind, &right.path, &right.old_path))
    });
    changes
}

fn reconcile(
    before: &Snapshot,
    after: &Snapshot,
    native_records: &[NativeRecord],
) -> (Vec<CanonicalEvent>, Vec<Diagnostic>) {
    let native_events = canonicalize_native_records(native_records);
    let changes = snapshot_diff(before, after);
    let mut matched_native = BTreeSet::new();
    let mut events = Vec::new();
    for change in changes {
        let matching = native_events.iter().enumerate().find(|(index, event)| {
            if matched_native.contains(index) {
                return false;
            }
            same_change(event, &change)
        });
        let (source, confidence, reason) = if let Some((index, _)) = matching {
            matched_native.insert(index);
            (
                "native+snapshot".to_owned(),
                "reconciled".to_owned(),
                Some("native event confirmed by a bounded snapshot".to_owned()),
            )
        } else {
            (
                "snapshot".to_owned(),
                "reconciled".to_owned(),
                Some("change found by bounded snapshot reconciliation".to_owned()),
            )
        };
        events.push(CanonicalEvent {
            record_type: "event".to_owned(),
            sequence: 0,
            kind: change.kind,
            path: change.path,
            old_path: change.old_path,
            source,
            confidence,
            reason,
        });
    }
    let mut diagnostics = Vec::new();
    for (index, event) in native_events.into_iter().enumerate() {
        if matched_native.contains(&index) {
            continue;
        }
        events.push(CanonicalEvent {
            record_type: "event".to_owned(),
            sequence: 0,
            kind: event.kind,
            path: event.path,
            old_path: event.old_path,
            source: "native".to_owned(),
            confidence: "unconfirmed".to_owned(),
            reason: Some("native event was not present in the final snapshot diff".to_owned()),
        });
        diagnostics.push(Diagnostic {
            severity: "degraded".to_owned(),
            code: "native-event-not-confirmed".to_owned(),
            detail: "a native notification was not confirmed by the bounded snapshot".to_owned(),
        });
    }
    if native_records.iter().any(|record| record.rescan_required) {
        diagnostics.push(Diagnostic {
            severity: "degraded".to_owned(),
            code: "native-rescan-required".to_owned(),
            detail: "the backend indicated that events may have been missed; the snapshot was used as the reconciliation authority".to_owned(),
        });
    }
    events.sort_by(|left, right| {
        (
            &left.kind,
            &left.path,
            &left.old_path,
            &left.source,
            &left.confidence,
        )
            .cmp(&(
                &right.kind,
                &right.path,
                &right.old_path,
                &right.source,
                &right.confidence,
            ))
    });
    for (index, event) in events.iter_mut().enumerate() {
        event.sequence = (index + 1) as u64;
    }
    (events, diagnostics)
}

fn same_change(event: &CanonicalEvent, change: &Change) -> bool {
    if event.kind == change.kind && event.path == change.path && event.old_path == change.old_path {
        return true;
    }
    change.kind == "Renamed"
        && ((event.kind == "Removed" && event.path == change.old_path.as_deref().unwrap_or(""))
            || (event.kind == "Created" && event.path == change.path))
}

fn canonicalize_native_records(records: &[NativeRecord]) -> Vec<CanonicalEvent> {
    let mut unique = BTreeMap::<(String, String, Option<String>), bool>::new();
    for record in records {
        let key = (
            record.kind.clone(),
            record.path.clone(),
            record.old_path.clone(),
        );
        let entry = unique.entry(key).or_insert(false);
        *entry |= record.rescan_required;
    }
    unique
        .into_iter()
        .enumerate()
        .map(
            |(index, ((kind, path, old_path), rescan_required))| CanonicalEvent {
                record_type: "event".to_owned(),
                sequence: (index + 1) as u64,
                kind,
                path,
                old_path,
                source: "native".to_owned(),
                confidence: "unconfirmed".to_owned(),
                reason: rescan_required.then_some("backend requested a rescan".to_owned()),
            },
        )
        .collect()
}

fn native_records_from_event(root: &Path, event: &Event) -> Vec<NativeRecord> {
    let Some(kind) = native_event_kind(event) else {
        return Vec::new();
    };
    let rescan_required = event.need_rescan();
    let paths = event
        .paths
        .iter()
        .map(|path| relative_path(root, path))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Vec::new();
    }
    if kind == "Renamed" && paths.len() >= 2 {
        return vec![NativeRecord {
            kind: kind.to_owned(),
            path: paths[1].clone(),
            old_path: Some(paths[0].clone()),
            rescan_required,
        }];
    }
    paths
        .into_iter()
        .map(|path| NativeRecord {
            kind: kind.to_owned(),
            path,
            old_path: None,
            rescan_required,
        })
        .collect()
}

fn native_event_kind(event: &Event) -> Option<&'static str> {
    match &event.kind {
        EventKind::Create(_) => Some("Created"),
        EventKind::Remove(_) => Some("Removed"),
        EventKind::Modify(ModifyKind::Name(_)) if event.paths.len() >= 2 => Some("Renamed"),
        EventKind::Modify(_) => Some("Modified"),
        EventKind::Any | EventKind::Other => Some("Modified"),
        EventKind::Access(_) => None,
    }
}

fn start_watcher(
    path: &Path,
    backend: BackendMode,
    poll_interval: Duration,
    sender: &Sender<notify::Result<Event>>,
) -> Result<WatcherState, FsError> {
    match backend {
        BackendMode::Poll => start_poll_watcher(path, poll_interval, sender),
        BackendMode::Native => {
            let native_sender = sender.clone();
            match notify::recommended_watcher(move |result| {
                let _ = native_sender.send(result);
            }) {
                Ok(mut watcher) => match watcher.watch(path, RecursiveMode::Recursive) {
                    Ok(()) => Ok(WatcherState {
                        _runtime: RuntimeWatcher::Native { _watcher: watcher },
                        selected: native_backend_name().to_owned(),
                        fallback_reason: None,
                    }),
                    Err(error) => start_poll_watcher_with_reason(
                        path,
                        poll_interval,
                        sender,
                        error.to_string(),
                    ),
                },
                Err(error) => {
                    start_poll_watcher_with_reason(path, poll_interval, sender, error.to_string())
                }
            }
        }
    }
}

fn start_poll_watcher(
    path: &Path,
    poll_interval: Duration,
    sender: &Sender<notify::Result<Event>>,
) -> Result<WatcherState, FsError> {
    start_poll_watcher_with_reason(path, poll_interval, sender, String::new())
}

fn start_poll_watcher_with_reason(
    path: &Path,
    poll_interval: Duration,
    sender: &Sender<notify::Result<Event>>,
    fallback_reason: String,
) -> Result<WatcherState, FsError> {
    let poll_sender = sender.clone();
    let mut watcher = PollWatcher::new(
        move |result| {
            let _ = poll_sender.send(result);
        },
        Config::default().with_poll_interval(poll_interval),
    )?;
    watcher.watch(path, RecursiveMode::Recursive)?;
    Ok(WatcherState {
        _runtime: RuntimeWatcher::Poll { _watcher: watcher },
        selected: "poll".to_owned(),
        fallback_reason: (!fallback_reason.is_empty()).then_some(fallback_reason),
    })
}

fn watcher_diagnostics(watcher: &WatcherState) -> Vec<Diagnostic> {
    watcher
        .fallback_reason
        .as_ref()
        .map(|reason| {
            vec![Diagnostic {
                severity: "degraded".to_owned(),
                code: "native-fallback".to_owned(),
                detail: format!("native watcher unavailable; polling fallback selected: {reason}"),
            }]
        })
        .unwrap_or_default()
}

fn backend_info(
    requested: BackendMode,
    selected: &str,
    filesystem: &FilesystemInfo,
    fallback_reason: Option<&str>,
) -> BackendInfo {
    let mut capabilities = vec!["bounded recursive snapshots".to_owned()];
    let mut limitations = vec![
        "native notifications are evidence of observed activity, not a completeness guarantee"
            .to_owned(),
        "snapshot cost is bounded by max entries and max bytes".to_owned(),
    ];
    if selected == "poll" {
        capabilities.push("periodic full-tree polling".to_owned());
        limitations
            .push("polling can miss changes that occur and disappear between scans".to_owned());
    } else {
        capabilities.push("native notifications via notify".to_owned());
        limitations
            .push("native queue overflow or backend limitations require reconciliation".to_owned());
    }
    if filesystem.likely_network_or_virtual {
        limitations.push(
            "the filesystem classification indicates network, overlay, pseudo, or virtual behavior; native completeness is not assumed"
                .to_owned(),
        );
    }
    if let Some(reason) = fallback_reason {
        limitations.push(format!("native backend fallback reason: {reason}"));
    }
    BackendInfo {
        requested: requested.as_str().to_owned(),
        selected: selected.to_owned(),
        guarantee: "best-effort observation with bounded snapshot reconciliation".to_owned(),
        capabilities,
        limitations,
    }
}

fn filesystem_info(path: &Path) -> FilesystemInfo {
    #[cfg(target_os = "linux")]
    let mount = find_mount(path);
    #[cfg(not(target_os = "linux"))]
    let mount: Option<(String, String)> = None;
    let kind = mount
        .as_ref()
        .map(|(_, kind)| kind.clone())
        .unwrap_or_else(|| "unknown".to_owned());
    let classification = classify_filesystem(&kind);
    let likely_network_or_virtual = classification != "local";
    let mut capabilities = vec!["bounded metadata snapshot".to_owned()];
    let mut limitations = vec![
        "filesystem type detection is a local hint and does not establish event completeness"
            .to_owned(),
    ];
    if likely_network_or_virtual {
        limitations.push(
            "network, pseudo, overlay, or virtual filesystems may omit or transform native events"
                .to_owned(),
        );
    }
    #[cfg(not(target_os = "linux"))]
    limitations.push("Linux mount metadata is unavailable on this target".to_owned());
    capabilities.shrink_to_fit();
    limitations.shrink_to_fit();
    FilesystemInfo {
        kind,
        classification,
        likely_network_or_virtual,
        mount_point: mount.map(|(mount_point, _)| redact_absolute_path(&mount_point)),
        capabilities,
        limitations,
    }
}

fn classify_filesystem(kind: &str) -> String {
    if kind == "unknown" {
        return "unknown".to_owned();
    }
    let network = ["nfs", "nfs4", "cifs", "smb3", "9p", "sshfs", "fuse.sshfs"];
    if network.contains(&kind) {
        return "network".to_owned();
    }
    let virtual_filesystems = [
        "overlay", "tmpfs", "proc", "sysfs", "cgroup", "cgroup2", "fuse", "fuseblk", "virtiofs",
        "vboxsf", "ramfs", "devtmpfs",
    ];
    if virtual_filesystems.contains(&kind) || kind.starts_with("fuse.") {
        return "virtual-or-pseudo".to_owned();
    }
    "local".to_owned()
}

#[cfg(target_os = "linux")]
fn find_mount(path: &Path) -> Option<(String, String)> {
    let contents = fs::read_to_string("/proc/self/mountinfo").ok()?;
    let mut best: Option<(PathBuf, String)> = None;
    for line in contents.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let fields = left.split_whitespace().collect::<Vec<_>>();
        let right_fields = right.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 5 || right_fields.is_empty() {
            continue;
        }
        let mount_point = PathBuf::from(decode_mountinfo_path(fields[4]));
        if path.starts_with(&mount_point)
            && best.as_ref().is_none_or(|(current, _)| {
                mount_point.components().count() > current.components().count()
            })
        {
            best = Some((mount_point, right_fields[0].to_owned()));
        }
    }
    best.map(|(mount_point, kind)| (mount_point.to_string_lossy().into_owned(), kind))
}

#[cfg(target_os = "linux")]
fn decode_mountinfo_path(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\134", "\\")
}

#[cfg(target_os = "linux")]
fn native_backend_name() -> &'static str {
    "inotify"
}

#[cfg(not(target_os = "linux"))]
fn native_backend_name() -> &'static str {
    "notify-native"
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| {
            if relative.as_os_str().is_empty() {
                ".".to_owned()
            } else {
                relative.to_string_lossy().replace('\\', "/")
            }
        })
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

fn modified_ns(metadata: &fs::Metadata) -> Option<i128> {
    metadata
        .modified()
        .ok()
        .map(|time| match time.duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos() as i128,
            Err(error) => -(error.duration().as_nanos() as i128),
        })
}

#[cfg(unix)]
fn permissions_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn permissions_mode(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

fn redact_absolute_path(value: &str) -> String {
    if value.starts_with('/') {
        let name = Path::new(value)
            .file_name()
            .map(|item| item.to_string_lossy().into_owned())
            .unwrap_or_else(|| "path".to_owned());
        format!("<absolute>/{name}")
    } else {
        value.to_owned()
    }
}

fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        (&left.severity, &left.code, &left.detail).cmp(&(
            &right.severity,
            &right.code,
            &right.detail,
        ))
    });
}

fn append_diagnostics(output: &mut String, diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        output.push_str("diagnostics: none\n");
    } else {
        output.push_str("diagnostics:\n");
        for diagnostic in diagnostics {
            let _ = writeln!(
                output,
                "- severity={} code={} detail={}",
                diagnostic.severity, diagnostic.code, diagnostic.detail
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, io::Read, thread};
    use tempfile::{TempDir, tempdir};

    fn fixture_dir() -> TempDir {
        tempdir().expect("tempdir")
    }

    fn write_and_verify(path: &Path, contents: &[u8]) {
        fs::write(path, contents).expect("write fixture");
        let mut file = File::open(path).expect("open fixture");
        let mut read_back = Vec::new();
        file.read_to_end(&mut read_back).expect("read fixture");
        assert_eq!(read_back, contents);
    }

    #[test]
    fn snapshot_reconciles_create_modify_rename_and_remove() {
        let directory = fixture_dir();
        let limits = ScanLimits::default();
        let empty = scan_snapshot(directory.path(), &limits).expect("empty snapshot");
        let first_path = directory.path().join("first.txt");
        write_and_verify(&first_path, b"one");
        let created = scan_snapshot(directory.path(), &limits).expect("created snapshot");
        let changes = snapshot_diff(&empty, &created);
        assert_eq!(changes[0].kind, "Created");
        assert_eq!(changes[0].path, "first.txt");

        write_and_verify(&first_path, b"two");
        let modified = scan_snapshot(directory.path(), &limits).expect("modified snapshot");
        let changes = snapshot_diff(&created, &modified);
        assert_eq!(changes[0].kind, "Modified");

        let renamed_path = directory.path().join("renamed.txt");
        fs::rename(&first_path, &renamed_path).expect("rename fixture");
        assert!(renamed_path.exists());
        let renamed = scan_snapshot(directory.path(), &limits).expect("renamed snapshot");
        let changes = snapshot_diff(&modified, &renamed);
        let rename = changes
            .iter()
            .find(|change| change.kind == "Renamed")
            .expect("rename change");
        assert_eq!(rename.old_path.as_deref(), Some("first.txt"));

        fs::remove_file(&renamed_path).expect("remove fixture");
        assert!(!renamed_path.exists());
        let removed = scan_snapshot(directory.path(), &limits).expect("removed snapshot");
        let changes = snapshot_diff(&renamed, &removed);
        assert!(changes.iter().any(|change| change.kind == "Removed"));
    }

    #[test]
    fn rename_storm_reconciles_each_unique_pair() {
        let directory = fixture_dir();
        let limits = ScanLimits::default();
        for index in 0..64 {
            let contents = vec![index as u8; index + 1];
            write_and_verify(
                &directory.path().join(format!("old-{index}.txt")),
                &contents,
            );
        }
        let before = scan_snapshot(directory.path(), &limits).expect("before rename storm");
        for index in 0..64 {
            fs::rename(
                directory.path().join(format!("old-{index}.txt")),
                directory.path().join(format!("new-{index}.txt")),
            )
            .expect("rename storm item");
        }
        for index in 0..64 {
            assert!(directory.path().join(format!("new-{index}.txt")).is_file());
            assert!(!directory.path().join(format!("old-{index}.txt")).exists());
        }
        let after = scan_snapshot(directory.path(), &limits).expect("after rename storm");
        let changes = snapshot_diff(&before, &after);
        assert_eq!(
            changes
                .iter()
                .filter(|change| change.kind == "Renamed")
                .count(),
            64
        );
    }

    #[test]
    fn duplicate_native_events_are_deduplicated() {
        let records = vec![
            NativeRecord {
                kind: "Modified".to_owned(),
                path: "a.txt".to_owned(),
                old_path: None,
                rescan_required: false,
            },
            NativeRecord {
                kind: "Modified".to_owned(),
                path: "a.txt".to_owned(),
                old_path: None,
                rescan_required: true,
            },
        ];
        let events = canonicalize_native_records(&records);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].reason.as_deref(),
            Some("backend requested a rescan")
        );
    }

    #[test]
    fn rescan_marker_and_unconfirmed_native_event_are_reported() {
        let before = BTreeMap::new();
        let after = BTreeMap::new();
        let records = vec![NativeRecord {
            kind: "Modified".to_owned(),
            path: "transient.txt".to_owned(),
            old_path: None,
            rescan_required: true,
        }];
        let (events, diagnostics) = reconcile(&before, &after, &records);
        assert_eq!(events[0].confidence, "unconfirmed");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "native-rescan-required")
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity == "degraded")
        );
    }

    #[test]
    fn queue_saturation_trace_is_deduplicated_and_degraded() {
        let records = (0..2048)
            .map(|_| NativeRecord {
                kind: "Modified".to_owned(),
                path: "burst.txt".to_owned(),
                old_path: None,
                rescan_required: true,
            })
            .collect::<Vec<_>>();
        let (events, diagnostics) = reconcile(&BTreeMap::new(), &BTreeMap::new(), &records);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].confidence, "unconfirmed");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "native-rescan-required")
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity == "degraded")
        );
    }

    #[cfg(unix)]
    #[test]
    fn permission_and_symlink_changes_are_snapshotted() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let directory = fixture_dir();
        let limits = ScanLimits::default();
        let file_path = directory.path().join("mode.txt");
        write_and_verify(&file_path, b"mode");
        let before = scan_snapshot(directory.path(), &limits).expect("before mode");
        let mut permissions = fs::metadata(&file_path).expect("metadata").permissions();
        let original_mode = permissions.mode();
        permissions.set_mode(original_mode ^ 0o100);
        fs::set_permissions(&file_path, permissions).expect("set permissions");
        let after = scan_snapshot(directory.path(), &limits).expect("after mode");
        assert!(
            snapshot_diff(&before, &after)
                .iter()
                .any(|change| { change.kind == "Modified" && change.path == "mode.txt" })
        );

        let target_a = directory.path().join("target-a");
        let target_b = directory.path().join("target-b");
        write_and_verify(&target_a, b"a");
        write_and_verify(&target_b, b"b");
        let link = directory.path().join("link");
        symlink("target-a", &link).expect("symlink a");
        let before_link = scan_snapshot(directory.path(), &limits).expect("before link");
        fs::remove_file(&link).expect("remove link");
        symlink("target-b", &link).expect("symlink b");
        let after_link = scan_snapshot(directory.path(), &limits).expect("after link");
        assert!(
            snapshot_diff(&before_link, &after_link)
                .iter()
                .any(|change| { change.kind == "Modified" && change.path == "link" })
        );
    }

    #[test]
    fn deterministic_replay_has_stable_json() {
        let before = BTreeMap::new();
        let mut after = BTreeMap::new();
        after.insert(
            "b.txt".to_owned(),
            Fingerprint {
                kind: EntryKind::File,
                bytes: 1,
                modified_ns: Some(1),
                permissions: 0,
                link_target: None,
            },
        );
        after.insert(
            "a.txt".to_owned(),
            Fingerprint {
                kind: EntryKind::File,
                bytes: 1,
                modified_ns: Some(1),
                permissions: 0,
                link_target: None,
            },
        );
        let first = reconcile(&before, &after, &[]).0;
        let second = reconcile(&before, &after, &[]).0;
        assert_eq!(
            render_json(&first).expect("json"),
            render_json(&second).expect("json")
        );
        assert_eq!(first[0].path, "a.txt");
    }

    #[test]
    fn polling_watcher_reconciles_a_real_change() {
        let directory = fixture_dir();
        let path = directory.path().to_path_buf();
        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            write_and_verify(&path.join("created.txt"), b"created");
        });
        let report = verify(
            directory.path(),
            Duration::from_millis(350),
            &WatchOptions {
                backend: BackendMode::Poll,
                poll_interval: Duration::from_millis(20),
                ..WatchOptions::default()
            },
        )
        .expect("poll verification");
        writer.join().expect("writer");
        assert_eq!(report.status, "ok");
        assert!(
            report
                .canonical_events
                .iter()
                .any(|event| { event.kind == "Created" && event.path == "created.txt" })
        );
    }

    #[test]
    fn filesystem_classification_is_present_for_available_mounts() {
        let root = validate_path(Path::new("/tmp")).expect("tmp");
        let info = filesystem_info(&root);
        assert!(!info.kind.is_empty());
        if Path::new("/dev/shm").exists() {
            let shm = validate_path(Path::new("/dev/shm")).expect("shm");
            let info = filesystem_info(&shm);
            assert!(!info.kind.is_empty());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn scans_available_local_and_tmpfs_mounts() {
        for mount_point in [Path::new("/tmp"), Path::new("/dev/shm")] {
            if !mount_point.exists() {
                continue;
            }
            let directory = tempfile::Builder::new()
                .prefix("fsproof-mount-")
                .tempdir_in(mount_point)
                .expect("mount fixture");
            let file_path = directory.path().join("file.txt");
            write_and_verify(&file_path, b"mount fixture");
            let snapshot =
                scan_snapshot(directory.path(), &ScanLimits::default()).expect("mount snapshot");
            assert!(snapshot.contains_key("file.txt"));
        }
    }

    #[test]
    fn bounded_snapshot_rejects_entry_and_byte_overflow() {
        let directory = fixture_dir();
        write_and_verify(&directory.path().join("a"), b"1234");
        let error = scan_snapshot(
            directory.path(),
            &ScanLimits {
                max_entries: 1,
                max_bytes: DEFAULT_MAX_BYTES,
            },
        )
        .expect_err("entry limit");
        assert!(error.to_string().contains("max entries"));
        let error = scan_snapshot(
            directory.path(),
            &ScanLimits {
                max_entries: DEFAULT_MAX_ENTRIES,
                max_bytes: 1,
            },
        )
        .expect_err("byte limit");
        assert!(error.to_string().contains("max bytes"));
    }

    #[test]
    fn duration_parser_requires_supported_units() {
        assert_eq!(
            parse_duration("10s").expect("seconds"),
            Duration::from_secs(10)
        );
        assert_eq!(
            parse_duration("25ms").expect("milliseconds"),
            Duration::from_millis(25)
        );
        assert!(parse_duration("10").is_err());
        assert!(parse_duration("10h").is_err());
    }
}
