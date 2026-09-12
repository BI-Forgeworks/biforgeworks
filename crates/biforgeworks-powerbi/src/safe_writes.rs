//! Explicit, snapshot-validated project transactions.
//!
//! A [`ProjectSession`] opens a discovered project, records a whole-tree
//! [`ProjectSnapshot`], and hands out one [`ProjectTransaction`] at a time.
//! A transaction stages changes to *managed* files only, validates them,
//! re-checks the entire tree for external changes, and then replaces each
//! target atomically through a temporary file and `rename`, with the
//! original bytes staged in an on-disk journal so a failed save can be
//! rolled back.
//!
//! Guarantees and their limits:
//!
//! - **Per file**: replacement is atomic. A reader sees either the old file
//!   or the new one.
//! - **Across files**: not atomic. If the process dies between two
//!   replacements, the tree is left part-old and part-new, the journal
//!   survives, and the next [`ProjectSession::open`] reports
//!   [`RecoveryArtifacts`] and refuses to save until they are resolved.
//!   Recovery is deliberately manual; see [`RecoveryArtifacts`].
//! - **No-op**: staging a value that already matches writes nothing at all —
//!   no temporary files, no journal, no timestamp changes.
//! - **Unknown content**: never rewritten, never reformatted. Only the exact
//!   byte span of an edited value changes inside a managed file.
//! - **Rollback restores bytes, not files**: an undone replacement is written
//!   the same way the replacement was — a new file renamed into place — so a
//!   rolled-back file holds exactly its original bytes but has a new inode
//!   and new modification and change times. Restoring those is not possible
//!   in general (`ctime` cannot be set at all), so a rolled-back save is
//!   byte-identical, not indistinguishable. Comparisons across a failed save
//!   should treat every staged target as touched and check its content.
//! - **A replaced file is always a new inode**, including on a successful
//!   save: replacement is `rename`, not an in-place rewrite. Preservation
//!   checks over a successful save should exclude the staged targets and
//!   compare everything else byte for byte.

use crate::fs_linux::{self, segment_name, Dir, EntryKind, FileStat, FsError};
use crate::json_span;
use crate::metadata;
use crate::snapshot::{
    self, Categories, ChangeKind, EntryChange, EntryType, FileCategory, ProjectSnapshot,
    SnapshotDiff, SnapshotEntry, SnapshotError,
};
use crate::{
    discover_project, ComponentFormat, Diagnostic, DiagnosticCode as Code, PowerBiProjectSummary,
    Severity,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Directory holding recovery data while a save is in progress. It exists
/// only during a save, and its presence afterwards means a save was
/// interrupted.
pub const JOURNAL_DIR: &str = ".biforgeworks-save";
const JOURNAL_FILE: &str = "journal.json";
const ORIGINALS_DIR: &str = "originals";
const TEMP_PREFIX: &str = ".biforgeworks-tmp-";
/// Largest file this crate will stage or replace.
const MAX_MANAGED_FILE_BYTES: u64 = crate::MAX_METADATA_BYTES;

/// The `.pbip` property this crate can edit: `settings.enableAutoRecovery`,
/// a boolean in the published `fabric/pbip/pbipProperties` schema.
const AUTO_RECOVERY_PATH: &[&str] = &["settings", "enableAutoRecovery"];

/// Where a project sits in the edit/save lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectState {
    /// Snapshot matches disk; nothing staged.
    Clean,
    /// Changes are staged but not written.
    Dirty,
    /// A commit is in progress.
    Saving,
    /// The project changed outside this session; refuse to save.
    Conflict,
    /// A save failed, or recovery data is pending.
    Error,
}

/// Why a safe-write operation stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SafeWriteErrorKind {
    /// The project could not be opened or discovered well enough to edit.
    OpenFailed,
    /// The tree could not be recorded completely.
    SnapshotFailed,
    /// Not supported on this platform, or this target cannot be replaced
    /// while preserving its metadata.
    Unsupported,
    /// The requested edit does not apply to this document.
    StageRejected,
    /// Staged content failed validation before anything was written.
    ValidationFailed,
    /// The project changed outside this session. Nothing was written.
    Conflict,
    /// Another save for the same project is running in this process.
    ConcurrentSave,
    /// Recovery data from an interrupted save must be resolved first.
    RecoveryPending,
    /// The save failed before any file was replaced; originals are intact.
    WriteFailed,
    /// Some files were replaced, then the save failed and every replacement
    /// was restored.
    RolledBack,
    /// The save failed and could not be fully undone. Original bytes are
    /// retained in the journal; manual recovery is required.
    RecoveryRequired,
}

/// A failure with the diagnostics explaining it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SafeWriteError {
    pub kind: SafeWriteErrorKind,
    pub diagnostics: Vec<Diagnostic>,
}

impl SafeWriteError {
    fn new(kind: SafeWriteErrorKind, diagnostics: Vec<Diagnostic>) -> Self {
        Self { kind, diagnostics }
    }

    fn single(
        kind: SafeWriteErrorKind,
        code: Code,
        message: impl Into<String>,
        path: Option<String>,
    ) -> Self {
        Self::new(
            kind,
            vec![Diagnostic::new(Severity::Error, code, message, path)],
        )
    }
}

impl std::fmt::Display for SafeWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let summary = self
            .diagnostics
            .first()
            .map(|d| d.message.as_str())
            .unwrap_or("no further detail");
        write!(f, "{:?}: {summary}", self.kind)
    }
}

impl std::error::Error for SafeWriteError {}

/// Recovery data left by an interrupted save.
///
/// This crate never deletes or replays these artifacts automatically: it
/// cannot know whether the files were edited by something else since the
/// interruption. Resolve them by hand — the original bytes of each target
/// are in `originals_directory`, listed by `targets` — then remove
/// [`JOURNAL_DIR`] to unblock saving.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryArtifacts {
    pub journal_directory: String,
    pub originals_directory: String,
    /// Targets named by the journal, if it could be read.
    pub targets: Vec<RecoveryTarget>,
    /// True when the journal file itself was missing or unreadable, so the
    /// artifacts could not be interpreted.
    pub journal_unreadable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryTarget {
    pub path: String,
    pub original_file: String,
    pub original_sha256: String,
    pub replaced: bool,
}

/// A change waiting to be written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StagedChange {
    pub path: String,
    pub category: FileCategory,
    pub size_before: u64,
    pub size_after: u64,
}

/// Result of staging a value that may already be in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StageOutcome {
    /// The file will be rewritten on commit.
    Changed,
    /// The document already holds this value; nothing was staged.
    AlreadyMatches,
}

/// What a commit would write, confirmed against the current tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub targets: Vec<StagedChange>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Outcome of a successful commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitReceipt {
    /// Relative paths actually replaced, in write order.
    pub changed_files: Vec<String>,
    /// True when nothing was staged, so nothing at all was written.
    pub unchanged: bool,
    /// Discovery re-run after the save.
    pub summary: PowerBiProjectSummary,
    /// Snapshot taken after the save.
    pub snapshot: ProjectSnapshot,
    pub diagnostics: Vec<Diagnostic>,
}

/// Deterministic failure points, for tests that must exercise the failure
/// and rollback paths rather than only the happy path. Only test-support
/// builds can arrange one through `ProjectTransaction::inject_failure`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePoint {
    /// Right after staging, before any validation.
    AfterStaging,
    /// After original bytes are journaled, before any replacement.
    AfterOriginalsStaged,
    /// Part-way through writing a replacement's temporary file.
    DuringTempWrite,
    /// Immediately before the first `rename`.
    BeforeReplace,
    /// After the first of several `rename`s.
    AfterFirstReplace,
    /// After all replacements, during post-save rediscovery.
    PostValidation,
    /// While restoring originals during rollback.
    RollbackRestore,
}

/// An open project that can be inspected and edited transactionally.
pub struct ProjectSession {
    root: Dir,
    root_path: PathBuf,
    project_file: PathBuf,
    /// The `.pbip` file name, which is also its snapshot path.
    managed_file: String,
    summary: PowerBiProjectSummary,
    snapshot: ProjectSnapshot,
    categories: Categories,
    state: ProjectState,
    recovery: Option<RecoveryArtifacts>,
}

impl ProjectSession {
    /// Opens the project whose `.pbip` file is at `path`.
    pub fn open(path: &Path) -> Result<ProjectSession, SafeWriteError> {
        let summary = discover_project(path);
        let fatal = [
            Code::PbipInvalidPath,
            Code::PbipInvalidExtension,
            Code::PbipNotFound,
            Code::PbipInvalidJson,
            Code::PbipInvalidStructure,
            Code::MetadataInvalidUtf8,
            Code::MetadataTooLarge,
            Code::NotARegularFile,
            Code::SymlinkRejected,
            Code::ReadOnlyGuaranteeUnavailable,
        ];
        if let Some(blocking) = summary
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error && fatal.contains(&d.code))
        {
            return Err(SafeWriteError::new(
                SafeWriteErrorKind::OpenFailed,
                vec![blocking.clone()],
            ));
        }

        let project_file = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let root_path = project_file
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let managed_file = project_file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| {
                SafeWriteError::single(
                    SafeWriteErrorKind::OpenFailed,
                    Code::PbipInvalidPath,
                    "The selected path does not name a .pbip file.",
                    None,
                )
            })?;
        let root = Dir::open_selected_root(&root_path).map_err(|error| {
            fs_error(
                SafeWriteErrorKind::OpenFailed,
                error,
                "The project folder",
                root_path.to_string_lossy().into_owned(),
            )
        })?;

        let categories = classify(&summary, &managed_file, &root_path);
        let recovery = detect_recovery(&root, &root_path)?;
        let snapshot = capture_snapshot(&root, &root_path, &categories, &BTreeSet::new())?;
        let state = if recovery.is_some() {
            ProjectState::Error
        } else {
            ProjectState::Clean
        };

        Ok(ProjectSession {
            root,
            root_path,
            project_file,
            managed_file,
            summary,
            snapshot,
            categories,
            state,
            recovery,
        })
    }

    /// Discovery result from when the session was opened or last committed.
    pub fn summary(&self) -> &PowerBiProjectSummary {
        &self.summary
    }

    /// Whole-tree snapshot from when the session was opened or last
    /// committed.
    pub fn snapshot(&self) -> &ProjectSnapshot {
        &self.snapshot
    }

    pub fn state(&self) -> ProjectState {
        self.state
    }

    pub fn project_file(&self) -> &Path {
        &self.project_file
    }

    /// Recovery data from an interrupted save, if any. While this is set,
    /// transactions are refused.
    pub fn pending_recovery(&self) -> Option<&RecoveryArtifacts> {
        self.recovery.as_ref()
    }

    /// Confirms the selected path still resolves to the very directory this
    /// session opened, so a renamed or swapped project root is caught rather
    /// than written to through the retained descriptor.
    fn verify_root(&self) -> Result<(), SafeWriteError> {
        let display = self.root_path.to_string_lossy().into_owned();
        let opened = self.root.stat_self().map_err(|error| {
            fs_error(
                SafeWriteErrorKind::Conflict,
                error,
                "The project folder",
                display.clone(),
            )
        })?;
        let current = Dir::open_selected_root(&self.root_path)
            .and_then(|dir| dir.stat_self())
            .map_err(|error| {
                fs_error(
                    SafeWriteErrorKind::Conflict,
                    error,
                    "The project folder",
                    display.clone(),
                )
            })?;
        if current.dev != opened.dev || current.ino != opened.ino {
            return Err(SafeWriteError::single(
                SafeWriteErrorKind::Conflict,
                Code::ExternalFileReplaced,
                "The project folder at the selected path is no longer the folder this session opened.",
                Some(display),
            ));
        }
        Ok(())
    }

    /// Re-runs discovery and re-captures the snapshot from disk.
    pub fn refresh(&mut self) -> Result<(), SafeWriteError> {
        self.verify_root()?;
        self.summary = discover_project(&self.project_file);
        self.categories = classify(&self.summary, &self.managed_file, &self.root_path);
        self.recovery = detect_recovery(&self.root, &self.root_path)?;
        self.snapshot = capture_snapshot(
            &self.root,
            &self.root_path,
            &self.categories,
            &BTreeSet::new(),
        )?;
        self.state = if self.recovery.is_some() {
            ProjectState::Error
        } else {
            ProjectState::Clean
        };
        Ok(())
    }

    /// Whether the project, as discovered, is one this crate will write to.
    ///
    /// A session opens for inspection even when discovery is unhappy, but a
    /// commit requires a sound baseline: both components resolved to known
    /// formats, and no discovery errors (which is where unsupported,
    /// escaping, or unreadable references surface).
    fn ensure_saveable(&self) -> Result<(), SafeWriteError> {
        let unsupported = |what: &str| {
            Err(SafeWriteError::single(
                SafeWriteErrorKind::ValidationFailed,
                Code::ProjectNotSaveable,
                format!(
                    "This project cannot be saved because its {what} is not in a supported state."
                ),
                Some(self.project_file.to_string_lossy().into_owned()),
            ))
        };
        let known = |format: ComponentFormat| {
            !matches!(format, ComponentFormat::Unknown | ComponentFormat::Missing)
        };
        if !self.summary.report.exists || !known(self.summary.report.format) {
            return unsupported("report");
        }
        if !self.summary.semantic_model.exists || !known(self.summary.semantic_model.format) {
            return unsupported("semantic model");
        }
        if let Some(error) = self
            .summary
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
        {
            return Err(SafeWriteError::new(
                SafeWriteErrorKind::ValidationFailed,
                vec![
                    Diagnostic::new(
                        Severity::Error,
                        Code::ProjectNotSaveable,
                        "This project reports discovery errors, so it will not be written to.",
                        Some(self.project_file.to_string_lossy().into_owned()),
                    ),
                    error.clone(),
                ],
            ));
        }
        Ok(())
    }

    /// Begins the session's transaction. Only one exists at a time, enforced
    /// by the mutable borrow.
    pub fn begin_transaction(&mut self) -> Result<ProjectTransaction<'_>, SafeWriteError> {
        if let Some(recovery) = &self.recovery {
            return Err(SafeWriteError::single(
                SafeWriteErrorKind::RecoveryPending,
                Code::RecoveryRequired,
                "A previous save was interrupted. Resolve the retained recovery data before saving again.",
                Some(recovery.journal_directory.clone()),
            ));
        }
        Ok(ProjectTransaction {
            session: self,
            staged: BTreeMap::new(),
            failure: None,
            before_rollback: None,
            after_replace: None,
            after_originals: None,
        })
    }
}

impl std::fmt::Debug for ProjectSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectSession")
            .field("project_file", &self.project_file)
            .field("state", &self.state)
            .field("entries", &self.snapshot.entries.len())
            .field("recovery_pending", &self.recovery.is_some())
            .finish()
    }
}

/// Staged changes for one save.
pub struct ProjectTransaction<'a> {
    session: &'a mut ProjectSession,
    staged: BTreeMap<String, StagedFile>,
    failure: Option<FailurePoint>,
    #[allow(clippy::type_complexity)]
    before_rollback: Option<Box<dyn Fn() + Send + Sync>>,
    #[allow(clippy::type_complexity)]
    after_replace: Option<Box<dyn Fn() + Send + Sync>>,
    after_originals: Option<Box<dyn Fn() + Send + Sync>>,
}

struct StagedFile {
    bytes: Vec<u8>,
    size_before: u64,
    category: FileCategory,
}

impl ProjectTransaction<'_> {
    /// Stages `settings.enableAutoRecovery` in the `.pbip` file.
    ///
    /// The document must already contain the property as a boolean; this
    /// crate does not add or restructure properties. Only that boolean's
    /// byte span changes, so every other byte — key order, indentation,
    /// newline style, byte order mark, trailing newline, and any property
    /// this crate does not know — is preserved exactly.
    pub fn stage_auto_recovery(&mut self, enabled: bool) -> Result<StageOutcome, SafeWriteError> {
        let outcome = self.stage_auto_recovery_inner(enabled);
        if matches!(&outcome, Err(error) if error.kind == SafeWriteErrorKind::Conflict) {
            self.session.state = ProjectState::Conflict;
        }
        outcome
    }

    fn stage_auto_recovery_inner(&mut self, enabled: bool) -> Result<StageOutcome, SafeWriteError> {
        let path = self.session.managed_file.clone();
        // The on-disk bytes are always re-verified against the snapshot, but
        // the edit applies to whatever this transaction last staged, so
        // repeated staging expresses the latest intent rather than stacking
        // on a stale read.
        let original = self.read_managed(&path)?;
        let bytes = match self.staged.get(&path) {
            Some(staged) => staged.bytes.clone(),
            None => original.clone(),
        };

        let document = metadata::parse_object(&bytes).map_err(|error| {
            SafeWriteError::single(
                SafeWriteErrorKind::StageRejected,
                Code::PbipInvalidJson,
                format!("The .pbip file {}.", error.describe()),
                Some(self.display(&path)),
            )
        })?;
        let current = document
            .get("settings")
            .and_then(|settings| settings.as_object())
            .and_then(|settings| settings.get("enableAutoRecovery"))
            .and_then(|value| value.as_bool())
            .ok_or_else(|| {
                SafeWriteError::single(
                    SafeWriteErrorKind::StageRejected,
                    Code::AutoRecoverySettingUnavailable,
                    "The .pbip file has no settings.enableAutoRecovery boolean to change; this crate does not add or restructure properties.",
                    Some(self.display(&path)),
                )
            })?;
        if current == enabled {
            return Ok(StageOutcome::AlreadyMatches);
        }

        let (span, located) = json_span::boolean_span(&bytes, AUTO_RECOVERY_PATH).map_err(|_| {
            SafeWriteError::single(
                SafeWriteErrorKind::StageRejected,
                Code::AutoRecoverySettingUnavailable,
                "The settings.enableAutoRecovery value could not be located as a single boolean token.",
                Some(self.display(&path)),
            )
        })?;
        if located != current {
            return Err(SafeWriteError::single(
                SafeWriteErrorKind::StageRejected,
                Code::AutoRecoverySettingUnavailable,
                "The located settings.enableAutoRecovery token does not agree with the parsed document.",
                Some(self.display(&path)),
            ));
        }
        let replacement = if enabled { &b"true"[..] } else { &b"false"[..] };
        let updated = json_span::splice(&bytes, span, replacement);
        self.verify_only_flag_changed(&bytes, &updated, enabled, &path)?;

        if updated == original {
            // Staged back to what is already on disk: nothing to write.
            self.staged.remove(&path);
            self.session.state = if self.staged.is_empty() {
                ProjectState::Clean
            } else {
                ProjectState::Dirty
            };
            return Ok(StageOutcome::AlreadyMatches);
        }
        self.staged.insert(
            path,
            StagedFile {
                bytes: updated,
                size_before: original.len() as u64,
                category: FileCategory::Managed,
            },
        );
        self.session.state = ProjectState::Dirty;
        Ok(StageOutcome::Changed)
    }

    /// Re-parses the edited bytes and confirms the only difference from the
    /// original document is the intended flag.
    fn verify_only_flag_changed(
        &self,
        before: &[u8],
        after: &[u8],
        expected: bool,
        path: &str,
    ) -> Result<(), SafeWriteError> {
        let reject = |message: &str| {
            SafeWriteError::single(
                SafeWriteErrorKind::StageRejected,
                Code::StagedContentInvalid,
                message.to_owned(),
                Some(self.display(path)),
            )
        };
        let mut original = metadata::parse_object(before).map_err(|_| reject("unparseable"))?;
        let mut updated = metadata::parse_object(after)
            .map_err(|_| reject("The edited .pbip file is not valid JSON."))?;
        for document in [&mut original, &mut updated] {
            if let Some(settings) = document.get_mut("settings").and_then(|s| s.as_object_mut()) {
                settings.remove("enableAutoRecovery");
            }
        }
        if original != updated {
            return Err(reject(
                "The edit would have changed more than settings.enableAutoRecovery.",
            ));
        }
        let written = metadata::parse_object(after)
            .ok()
            .and_then(|document| {
                document
                    .get("settings")?
                    .as_object()?
                    .get("enableAutoRecovery")?
                    .as_bool()
            })
            .ok_or_else(|| reject("The edited .pbip file lost its enableAutoRecovery boolean."))?;
        if written != expected {
            return Err(reject("The edited .pbip file holds the wrong value."));
        }
        Ok(())
    }

    /// Stages replacement bytes for an existing regular file. Test support
    /// only: production exposes no freeform write path.
    #[cfg(any(test, feature = "safe_write_test_support"))]
    #[doc(hidden)]
    pub fn stage_replacement_for_tests(
        &mut self,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<StageOutcome, SafeWriteError> {
        let existing = self.read_managed(path)?;
        if existing == bytes {
            return Ok(StageOutcome::AlreadyMatches);
        }
        let category = self
            .session
            .snapshot
            .entry(path)
            .map(|entry| entry.category)
            .unwrap_or(FileCategory::Unknown);
        self.staged.insert(
            path.to_owned(),
            StagedFile {
                bytes,
                size_before: existing.len() as u64,
                category,
            },
        );
        self.session.state = ProjectState::Dirty;
        Ok(StageOutcome::Changed)
    }

    /// Arranges a deterministic failure at `point` during the next commit.
    #[cfg(any(test, feature = "safe_write_test_support"))]
    #[doc(hidden)]
    pub fn inject_failure(&mut self, point: FailurePoint) {
        self.failure = Some(point);
    }

    /// Runs `hook` immediately after each file is renamed into place, so
    /// tests can make an external change at exactly that point.
    #[cfg(any(test, feature = "safe_write_test_support"))]
    #[doc(hidden)]
    pub fn on_after_replace(&mut self, hook: Box<dyn Fn() + Send + Sync>) {
        self.after_replace = Some(hook);
    }

    /// Runs after original backups are durable and before the conflict recheck.
    #[cfg(any(test, feature = "safe_write_test_support"))]
    #[doc(hidden)]
    pub fn on_after_originals_staged(&mut self, hook: Box<dyn Fn() + Send + Sync>) {
        self.after_originals = Some(hook);
    }

    /// Runs `hook` once, immediately before each file is restored during
    /// rollback, so tests can make an external change at exactly that point
    /// instead of racing a thread against it.
    #[cfg(any(test, feature = "safe_write_test_support"))]
    #[doc(hidden)]
    pub fn on_before_rollback(&mut self, hook: Box<dyn Fn() + Send + Sync>) {
        self.before_rollback = Some(hook);
    }

    pub fn staged(&self) -> Vec<StagedChange> {
        self.staged
            .iter()
            .map(|(path, file)| StagedChange {
                path: path.clone(),
                category: file.category,
                size_before: file.size_before,
                size_after: file.bytes.len() as u64,
            })
            .collect()
    }

    /// Checks staged content and destinations without writing anything.
    pub fn validate(&self) -> Result<ValidationReport, SafeWriteError> {
        let mut diagnostics = Vec::new();
        for (path, file) in &self.staged {
            self.validate_target(path, file, &mut diagnostics)?;
        }
        Ok(ValidationReport {
            targets: self.staged(),
            diagnostics,
        })
    }

    /// Discards staged changes. Nothing has touched the filesystem, and the
    /// session returns to `Clean` (see the `Drop` implementation).
    pub fn cancel(self) {}

    /// Validates, checks for external changes, and writes staged files.
    pub fn commit(mut self) -> Result<CommitReceipt, SafeWriteError> {
        let outcome = self.commit_inner();
        // The transaction is consumed either way, so its staged payload is
        // gone: the session is never left `Dirty` with nothing retained.
        self.staged.clear();
        self.session.state = match &outcome {
            Ok(_) => ProjectState::Clean,
            Err(error) => match error.kind {
                SafeWriteErrorKind::Conflict => ProjectState::Conflict,
                _ => ProjectState::Error,
            },
        };
        outcome
    }

    fn commit_inner(&mut self) -> Result<CommitReceipt, SafeWriteError> {
        self.session.ensure_saveable()?;
        self.session.verify_root()?;
        let identity = self.session.root.stat_self().map_err(|error| {
            fs_error(
                SafeWriteErrorKind::WriteFailed,
                error,
                "The project folder",
                self.session.root_path.to_string_lossy().into_owned(),
            )
        })?;
        let _lock = SaveLock::acquire(identity.dev, identity.ino).ok_or_else(|| {
            SafeWriteError::single(
                SafeWriteErrorKind::ConcurrentSave,
                Code::ConcurrentSaveBlocked,
                "Another save for this project is already in progress.",
                Some(self.session.root_path.to_string_lossy().into_owned()),
            )
        })?;
        self.session.state = ProjectState::Saving;

        if self.failure == Some(FailurePoint::AfterStaging) {
            return Err(injected(SafeWriteErrorKind::WriteFailed, "after staging"));
        }
        self.validate()?;

        // Re-observe the whole tree — managed, preserved, and unknown alike.
        // Nothing is ignored here: the journal does not exist yet, so an
        // externally created one is a genuine conflict.
        let before_write = self.capture_with(&BTreeSet::new())?;
        let diff = self.session.snapshot.diff(&before_write, &BTreeSet::new());
        if !diff.is_empty() {
            return Err(conflict_error(&diff));
        }
        if self.staged.is_empty() {
            // No-op: no journal, no temporary file, no write of any kind.
            self.session.snapshot = before_write;
            return Ok(CommitReceipt {
                changed_files: Vec::new(),
                unchanged: true,
                summary: self.session.summary.clone(),
                snapshot: self.session.snapshot.clone(),
                diagnostics: Vec::new(),
            });
        }

        let plan = self.plan(&before_write)?;
        let mut journal = Journal::create(&self.session.root, &self.session.root_path)?;
        // From here on, only entries this transaction created are excluded
        // from comparisons.
        let owned = BTreeSet::from([JOURNAL_DIR.to_owned()]);

        let replaced = match self.write_all(&plan, &mut journal, &before_write, &owned) {
            Ok(replaced) => replaced,
            Err((problem, replaced)) => {
                if replaced.is_empty() {
                    // Nothing was replaced, so every original is untouched.
                    let kind = problem.kind;
                    let mut diagnostics = problem.diagnostics;
                    diagnostics.extend(journal.finish(&self.session.root));
                    return Err(SafeWriteError::new(kind, diagnostics));
                }
                return Err(self.roll_back(&plan, &replaced, &mut journal, problem));
            }
        };

        // Everything below runs while the journal, and therefore every
        // original, is still on disk: any failure can still be undone. Every
        // fallible step happens here, before cleanup, so that once the
        // originals are gone nothing is left that could still fail.
        let after_summary = match self.post_validate() {
            Ok(summary) => summary,
            Err(problem) => return Err(self.roll_back(&plan, &replaced, &mut journal, problem)),
        };
        let after_write = match self.capture_with(&owned) {
            Ok(after) => after,
            Err(problem) => return Err(self.roll_back(&plan, &replaced, &mut journal, problem)),
        };
        if let Err(problem) =
            expected_changes_only(&before_write, &after_write, &plan, &replaced, &owned)
        {
            return Err(self.roll_back(&plan, &replaced, &mut journal, problem));
        }

        // The save is confirmed. The summary and snapshot above are the ones
        // that were validated, so they become the session's state directly.
        self.session.summary = after_summary;
        self.session.categories = classify(
            &self.session.summary,
            &self.session.managed_file,
            &self.session.root_path,
        );
        self.session.snapshot = after_write;

        // Only now are the staged originals removed.
        let cleanup = journal.finish(&self.session.root);
        if !cleanup.is_empty() {
            // The save itself is complete and validated; what failed is
            // housekeeping. The retained data is evidence of a partial
            // cleanup, not a guaranteed set of originals, so it is never
            // presented as something to restore from.
            let mut diagnostics = cleanup;
            diagnostics.push(Diagnostic::new(
                Severity::Error,
                Code::RecoveryArtifactsRetained,
                "The staged files were saved and verified, but the recovery data could not be fully removed. What remains is partial cleanup evidence, not a usable set of originals; inspect and remove it by hand.",
                Some(journal.directory_display.clone()),
            ));
            journal.retain();
            return Err(SafeWriteError::new(
                SafeWriteErrorKind::RecoveryRequired,
                diagnostics,
            ));
        }

        Ok(CommitReceipt {
            changed_files: replaced.iter().map(|write| write.path.clone()).collect(),
            unchanged: false,
            summary: self.session.summary.clone(),
            snapshot: self.session.snapshot.clone(),
            diagnostics: Vec::new(),
        })
    }

    /// Confirms each target is a replaceable regular file whose metadata can
    /// be carried across a rename, and that it still matches the snapshot.
    fn plan(&self, fresh: &ProjectSnapshot) -> Result<Vec<PlannedWrite>, SafeWriteError> {
        let (euid, _) = fs_linux::effective_ids();
        let mut plan = Vec::new();
        for (path, file) in &self.staged {
            let entry = fresh.entry(path).ok_or_else(|| {
                SafeWriteError::single(
                    SafeWriteErrorKind::Conflict,
                    Code::ExternalFileRemoved,
                    "A file staged for replacement no longer exists.",
                    Some(self.display(path)),
                )
            })?;
            let (parent, name) = self.resolve(path)?;
            let unsupported = |message: &str| {
                Err(SafeWriteError::single(
                    SafeWriteErrorKind::Unsupported,
                    Code::UnsafeWriteTarget,
                    message.to_owned(),
                    Some(self.display(path)),
                ))
            };
            let (handle, stat) = parent.open_regular(&name).map_err(|error| {
                fs_error(
                    SafeWriteErrorKind::Conflict,
                    error,
                    "A file staged for replacement",
                    self.display(path),
                )
            })?;
            if stat.nlink != 1 {
                return unsupported(
                    "The file has more than one hard link; replacing it would break the other links.",
                );
            }
            if stat.uid != euid {
                return unsupported(
                    "The file is owned by another user, so a replacement could not keep its ownership.",
                );
            }
            if fs_linux::has_extended_attributes(&handle).map_err(|error| {
                fs_error(
                    SafeWriteErrorKind::Unsupported,
                    error,
                    "The file's extended attributes",
                    self.display(path),
                )
            })? {
                return unsupported(
                    "The file carries extended attributes or an ACL, which a replacement cannot preserve.",
                );
            }
            drop(handle);

            let (original_bytes, _) = self.read_current(&parent, &name, path)?;
            if snapshot::sha256_hex(&original_bytes)
                != entry.content_sha256.clone().unwrap_or_default()
            {
                return Err(SafeWriteError::single(
                    SafeWriteErrorKind::Conflict,
                    Code::ExternalFileChanged,
                    "The file changed outside this session between the tree check and the save.",
                    Some(self.display(path)),
                ));
            }
            plan.push(PlannedWrite {
                path: path.clone(),
                parent,
                name,
                stat,
                expected: entry.clone(),
                new_bytes: file.bytes.clone(),
                new_sha256: snapshot::sha256_hex(&file.bytes),
                original_sha256: snapshot::sha256_hex(&original_bytes),
                original_bytes,
            });
        }
        Ok(plan)
    }

    /// Stages originals, then replaces each target, re-checking the root and
    /// the target itself immediately before every rename.
    fn write_all(
        &self,
        plan: &[PlannedWrite],
        journal: &mut Journal,
        before_write: &ProjectSnapshot,
        owned: &BTreeSet<String>,
    ) -> Result<Vec<Replaced>, (SafeWriteError, Vec<Replaced>)> {
        let mut replaced: Vec<Replaced> = Vec::new();
        journal
            .stage_originals(&self.session.root, plan)
            .map_err(|error| (error, Vec::new()))?;

        if let Some(hook) = &self.after_originals {
            hook();
        }

        // Staging originals can take a while on a large project, so the
        // whole tree is observed again here — excluding only the journal
        // this transaction just created — before anything is replaced.
        match self.capture_with(owned) {
            Ok(current) => {
                let diff = before_write.diff(&current, owned);
                if !diff.is_empty() {
                    return Err((conflict_error(&diff), replaced));
                }
            }
            Err(error) => return Err((error, replaced)),
        }
        if self.failure == Some(FailurePoint::AfterOriginalsStaged) {
            return Err((
                injected(SafeWriteErrorKind::WriteFailed, "after staging originals"),
                replaced,
            ));
        }

        for (index, write) in plan.iter().enumerate() {
            let partial = self.failure == Some(FailurePoint::DuringTempWrite);
            let (temp, prepared) = match self.write_temp(write, partial) {
                Ok(prepared) => prepared,
                Err(error) => return Err((error, replaced)),
            };
            let abort = |error: SafeWriteError, replaced: Vec<Replaced>| {
                let _ = write.parent.unlink_entry(&temp);
                Err((error, replaced))
            };
            if partial {
                return abort(
                    injected(
                        SafeWriteErrorKind::WriteFailed,
                        "during the temporary write",
                    ),
                    replaced,
                );
            }
            if self.failure == Some(FailurePoint::BeforeReplace) {
                return abort(
                    injected(
                        SafeWriteErrorKind::WriteFailed,
                        "before the first replacement",
                    ),
                    replaced,
                );
            }
            if let Err(error) = self.session.verify_root() {
                return abort(error, replaced);
            }
            // Re-check immediately before replacing, so a file changed since
            // the tree-wide observation is still caught.
            if let Err(error) = self.verify_unchanged(write) {
                return abort(error, replaced);
            }
            if let Err(error) = journal.mark_replacing(index) {
                return abort(error, replaced);
            }
            if let Err(error) = write.parent.rename_entry(&temp, &write.name) {
                return abort(
                    fs_error(
                        SafeWriteErrorKind::WriteFailed,
                        error,
                        "The replacement file",
                        self.display(&write.path),
                    ),
                    replaced,
                );
            }
            replaced.push(Replaced {
                path: write.path.clone(),
                installed: prepared,
            });
            // The name must now resolve to the very file just written. If
            // something replaced it in the meantime, that is not our write,
            // and rollback must not treat it as ours.
            match write.parent.stat_entry(&write.name) {
                Ok(Some(stat))
                    if stat.ino == prepared.ino
                        && stat.dev == prepared.dev
                        && stat.mode == prepared.mode
                        && stat.uid == prepared.uid
                        && stat.gid == prepared.gid
                        && stat.nlink == prepared.nlink => {}
                _ => {
                    return Err((
                        SafeWriteError::single(
                            SafeWriteErrorKind::Conflict,
                            Code::ExternalFileReplaced,
                            "The file was replaced by something else immediately after this save wrote it.",
                            Some(self.display(&write.path)),
                        ),
                        replaced,
                    ));
                }
            }
            if let Some(hook) = &self.after_replace {
                hook();
            }
            if let Err(error) = write.parent.sync() {
                return Err((
                    fs_error(
                        SafeWriteErrorKind::WriteFailed,
                        error,
                        "The folder holding the replaced file",
                        self.display(&write.path),
                    ),
                    replaced,
                ));
            }
            if self.failure == Some(FailurePoint::AfterFirstReplace) && replaced.len() == 1 {
                return Err((
                    injected(
                        SafeWriteErrorKind::WriteFailed,
                        "after the first replacement",
                    ),
                    replaced,
                ));
            }
        }
        Ok(replaced)
    }

    /// Writes `bytes` to a fresh temporary file in `write`'s own directory,
    /// reproducing the target's mode and group, flushed before it is
    /// renamed into place.
    fn write_temp_bytes(
        &self,
        write: &PlannedWrite,
        bytes: &[u8],
    ) -> Result<(CString, FileStat), SafeWriteError> {
        let mut last = FsError::AlreadyExists;
        for _ in 0..8 {
            let name = temp_name();
            let c_name = segment_name(name.as_bytes()).map_err(|error| {
                fs_error(
                    SafeWriteErrorKind::WriteFailed,
                    error,
                    "The temporary file name",
                    self.display(&write.path),
                )
            })?;
            match write.parent.create_new_file(&c_name, write.stat.mode) {
                Ok(mut file) => {
                    let prepared = file
                        .write_all(bytes)
                        .map_err(|_| FsError::Io)
                        .and_then(|()| fs_linux::set_mode(&file, write.stat.mode))
                        .and_then(|()| fs_linux::set_group(&file, write.stat.gid))
                        .and_then(|()| fs_linux::sync_file(&file))
                        // A directory with a default ACL grants one to every
                        // file created in it. The target had none (that is
                        // checked when planning), so a replacement that
                        // picked one up would quietly change who can read
                        // the file: refuse rather than install it.
                        .and_then(|()| match fs_linux::has_extended_attributes(&file) {
                            Ok(false) => Ok(()),
                            Ok(true) => Err(FsError::Unsupported),
                            Err(error) => Err(error),
                        })
                        .and_then(|()| fs_linux::stat_open_file(&file));
                    let failed = match &prepared {
                        Err(error) => Some(*error),
                        // The replacement must carry the target's mode and
                        // ownership, or it is not written at all.
                        Ok(stat)
                            if stat.mode != write.stat.mode
                                || stat.uid != write.stat.uid
                                || stat.gid != write.stat.gid =>
                        {
                            Some(FsError::Io)
                        }
                        Ok(_) => None,
                    };
                    if let Some(error) = failed {
                        drop(file);
                        let _ = write.parent.unlink_entry(&c_name);
                        return Err(fs_error(
                            SafeWriteErrorKind::WriteFailed,
                            error,
                            "The replacement file",
                            self.display(&write.path),
                        ));
                    }
                    // The identity comes from the descriptor this process
                    // wrote, before the file is exposed under the target
                    // name, so nothing appearing at that name afterwards can
                    // be mistaken for our own write.
                    let prepared = prepared.expect("checked just above");
                    return Ok((c_name, prepared));
                }
                Err(FsError::AlreadyExists) => last = FsError::AlreadyExists,
                Err(error) => {
                    return Err(fs_error(
                        SafeWriteErrorKind::WriteFailed,
                        error,
                        "The replacement file",
                        self.display(&write.path),
                    ))
                }
            }
        }
        Err(fs_error(
            SafeWriteErrorKind::WriteFailed,
            last,
            "A temporary file",
            self.display(&write.path),
        ))
    }

    fn write_temp(
        &self,
        write: &PlannedWrite,
        partial: bool,
    ) -> Result<(CString, FileStat), SafeWriteError> {
        let bytes = if partial {
            &write.new_bytes[..write.new_bytes.len() / 2]
        } else {
            &write.new_bytes[..]
        };
        self.write_temp_bytes(write, bytes)
    }

    /// Confirms a target still matches what the plan recorded, in content,
    /// identity, permissions, ownership, link count, and extended
    /// attributes.
    fn verify_unchanged(&self, write: &PlannedWrite) -> Result<(), SafeWriteError> {
        let conflict = |message: &str| {
            SafeWriteError::single(
                SafeWriteErrorKind::Conflict,
                Code::ExternalFileChanged,
                message.to_owned(),
                Some(self.display(&write.path)),
            )
        };
        let (bytes, stat) = self.read_current(&write.parent, &write.name, &write.path)?;
        let expected = &write.expected;
        let matches = snapshot::sha256_hex(&bytes) == write.original_sha256
            && stat.ino == expected.identity.inode
            && stat.dev == expected.identity.device
            && stat.mode == expected.mode
            && stat.uid == expected.uid
            && stat.gid == expected.gid
            && stat.nlink == expected.link_count;
        if !matches {
            return Err(conflict(
                "The file changed outside this session while the save was running; nothing further was written.",
            ));
        }
        let (handle, _) = write.parent.open_regular(&write.name).map_err(|error| {
            fs_error(
                SafeWriteErrorKind::Conflict,
                error,
                "A file staged for replacement",
                self.display(&write.path),
            )
        })?;
        if fs_linux::has_extended_attributes(&handle).map_err(|error| {
            fs_error(
                SafeWriteErrorKind::Conflict,
                error,
                "The file's extended attributes",
                self.display(&write.path),
            )
        })? {
            return Err(conflict(
                "The file gained extended attributes or an ACL while the save was running.",
            ));
        }
        Ok(())
    }

    /// Re-runs discovery and requires the project to still resolve the same
    /// components, formats, and diagnostics.
    fn post_validate(&self) -> Result<PowerBiProjectSummary, SafeWriteError> {
        // `RollbackRestore` also fails here, so that arranging a rollback
        // failure actually provokes the rollback it is meant to test.
        if matches!(
            self.failure,
            Some(FailurePoint::PostValidation) | Some(FailurePoint::RollbackRestore)
        ) {
            return Err(injected(
                SafeWriteErrorKind::ValidationFailed,
                "during post-save validation",
            ));
        }
        let after = discover_project(&self.session.project_file);
        let before = &self.session.summary;
        let mismatch = |what: &str| {
            SafeWriteError::single(
                SafeWriteErrorKind::ValidationFailed,
                Code::PostSaveValidationFailed,
                format!("After saving, the project no longer resolves the same {what}."),
                Some(self.session.project_file.to_string_lossy().into_owned()),
            )
        };
        if after.report != before.report {
            return Err(mismatch("report"));
        }
        if after.semantic_model != before.semantic_model {
            return Err(mismatch("semantic model"));
        }
        // Compare the diagnostics themselves, not how many there are.
        let codes = |summary: &PowerBiProjectSummary| -> Vec<String> {
            let mut codes: Vec<String> = summary
                .diagnostics
                .iter()
                .map(|d| format!("{:?}/{:?}", d.severity, d.code))
                .collect();
            codes.sort();
            codes
        };
        if codes(&after) != codes(before) {
            return Err(mismatch("set of diagnostics"));
        }
        Ok(after)
    }

    /// Restores originals for files already replaced, newest first, never
    /// overwriting a file that something else changed after our write.
    fn roll_back(
        &self,
        plan: &[PlannedWrite],
        replaced: &[Replaced],
        journal: &mut Journal,
        cause: SafeWriteError,
    ) -> SafeWriteError {
        let mut diagnostics = cause.diagnostics;
        let mut unresolved = Vec::new();

        for entry in replaced.iter().rev() {
            let Some(write) = plan.iter().find(|write| write.path == entry.path) else {
                unresolved.push(entry.path.clone());
                continue;
            };
            if let Some(hook) = &self.before_rollback {
                hook();
            }
            let restored = if self.failure == Some(FailurePoint::RollbackRestore) {
                Err(SafeWriteError::single(
                    SafeWriteErrorKind::RecoveryRequired,
                    Code::RollbackFailed,
                    "Injected rollback failure.",
                    Some(self.display(&entry.path)),
                ))
            } else {
                self.restore(write, entry)
            };
            match restored {
                Ok(()) => {}
                Err(error) => {
                    diagnostics.extend(error.diagnostics);
                    unresolved.push(entry.path.clone());
                }
            }
        }

        if unresolved.is_empty() {
            diagnostics.push(Diagnostic::new(
                Severity::Warning,
                Code::SaveRolledBack,
                "The save failed; every replaced file was restored from its staged original.",
                Some(self.session.root_path.to_string_lossy().into_owned()),
            ));
            diagnostics.extend(journal.finish(&self.session.root));
            return SafeWriteError::new(SafeWriteErrorKind::RolledBack, diagnostics);
        }

        journal.retain();
        diagnostics.push(Diagnostic::new(
            Severity::Error,
            Code::RecoveryRequired,
            format!(
                "The save failed and {} file(s) could not be restored. Original bytes are retained in {}; resolve them by hand before saving again.",
                unresolved.len(),
                journal.directory_display
            ),
            Some(journal.directory_display.clone()),
        ));
        SafeWriteError::new(SafeWriteErrorKind::RecoveryRequired, diagnostics)
    }

    /// Restores one original.
    ///
    /// The file is only overwritten while it still is, exactly, the file
    /// this save installed: same bytes, same inode, same mode, ownership and
    /// link count. That is checked once before the replacement is prepared
    /// and again immediately before the rename, so an external edit is not
    /// clobbered.
    ///
    /// The remaining gap is unavoidable without cooperation from the other
    /// writer: `rename` cannot be made conditional on the destination's
    /// contents, so a write landing in the instant between the final check
    /// and the rename can be lost. Checks narrow that window but cannot
    /// exclude an uncooperative external writer.
    fn restore(&self, write: &PlannedWrite, installed: &Replaced) -> Result<(), SafeWriteError> {
        self.session.verify_root()?;
        let refuse = |message: &str| {
            Err(SafeWriteError::single(
                SafeWriteErrorKind::RecoveryRequired,
                Code::RollbackFailed,
                message.to_owned(),
                Some(self.display(&write.path)),
            ))
        };
        let still_ours = |bytes: &[u8], stat: &FileStat| -> Result<bool, SafeWriteError> {
            let digest = snapshot::sha256_hex(bytes);
            if digest == write.original_sha256 {
                // Already back to the original bytes; leave it alone.
                return Ok(false);
            }
            if digest != write.new_sha256 {
                return Ok(false);
            }
            let expected = installed.installed;
            let metadata_matches = stat.ino == expected.ino
                && stat.dev == expected.dev
                && stat.mode == expected.mode
                && stat.uid == expected.uid
                && stat.gid == expected.gid
                && stat.nlink == expected.nlink;
            if !metadata_matches {
                return Ok(false);
            }
            // Attribute changes need not change mode or contents. Never drop
            // another writer's ACL or xattr while restoring original bytes.
            let attributes_unchanged =
                write
                    .parent
                    .open_regular(&write.name)
                    .is_ok_and(|(handle, observed)| {
                        observed.same_file(stat)
                            && matches!(fs_linux::has_extended_attributes(&handle), Ok(false))
                    });
            Ok(attributes_unchanged)
        };

        let (current, stat) = self.read_current(&write.parent, &write.name, &write.path)?;
        if snapshot::sha256_hex(&current) == write.original_sha256 {
            return Ok(());
        }
        if !still_ours(&current, &stat)? {
            return refuse(
                "The file was changed or replaced by something else after this save wrote it, so the original was not restored over it.",
            );
        }

        let (temp, _) = self
            .write_temp_bytes(write, &write.original_bytes)
            .map_err(|error| {
                SafeWriteError::new(SafeWriteErrorKind::RecoveryRequired, error.diagnostics)
            })?;
        let abandon = |error: SafeWriteError| {
            let _ = write.parent.unlink_entry(&temp);
            Err(error)
        };

        // Re-check immediately before the rename.
        let (again, stat) = match self.read_current(&write.parent, &write.name, &write.path) {
            Ok(current) => current,
            Err(error) => {
                return abandon(SafeWriteError::new(
                    SafeWriteErrorKind::RecoveryRequired,
                    error.diagnostics,
                ))
            }
        };
        match still_ours(&again, &stat) {
            Ok(true) => {}
            Ok(false) => {
                let _ = write.parent.unlink_entry(&temp);
                return refuse(
                    "The file changed while the original was being restored, so it was left as it is.",
                );
            }
            Err(error) => return abandon(error),
        }
        if let Err(error) = self.session.verify_root() {
            return abandon(error);
        }
        if let Err(error) = write.parent.rename_entry(&temp, &write.name) {
            return abandon(fs_error(
                SafeWriteErrorKind::RecoveryRequired,
                error,
                "The restored original",
                self.display(&write.path),
            ));
        }
        write.parent.sync().map_err(|error| {
            fs_error(
                SafeWriteErrorKind::RecoveryRequired,
                error,
                "The folder holding the restored original",
                self.display(&write.path),
            )
        })
    }

    fn validate_target(
        &self,
        path: &str,
        file: &StagedFile,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<(), SafeWriteError> {
        let reject = |code: Code, message: &str| {
            SafeWriteError::single(
                SafeWriteErrorKind::ValidationFailed,
                code,
                message.to_owned(),
                Some(self.display(path)),
            )
        };
        if file.bytes.len() as u64 > MAX_MANAGED_FILE_BYTES {
            return Err(reject(
                Code::MetadataTooLarge,
                "The staged content is larger than the supported metadata size.",
            ));
        }
        // Destinations come from project metadata, never from a caller path.
        if path.is_empty()
            || path.starts_with('/')
            || path
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == "..")
        {
            return Err(reject(
                Code::UnsafeWriteTarget,
                "The destination is not a safe relative path inside the project.",
            ));
        }
        let entry = self.session.snapshot.entry(path).ok_or_else(|| {
            reject(
                Code::UnsafeWriteTarget,
                "The destination is not a file recorded in the project snapshot.",
            )
        })?;
        if entry.entry_type != EntryType::File {
            return Err(reject(
                Code::UnsafeWriteTarget,
                "The destination is not a regular file.",
            ));
        }
        if entry.category == FileCategory::Preserved {
            return Err(reject(
                Code::UnsafeWriteTarget,
                "The destination is a preserved file that discovery depends on.",
            ));
        }
        if path == self.session.managed_file {
            metadata::parse_object(&file.bytes).map_err(|error| {
                reject(
                    Code::StagedContentInvalid,
                    &format!("The staged .pbip content {}.", error.describe()),
                )
            })?;
            if std::str::from_utf8(&file.bytes).is_err() {
                return Err(reject(
                    Code::StagedContentInvalid,
                    "The staged .pbip content is not valid UTF-8.",
                ));
            }
            let staged = metadata::parse_object(&file.bytes).unwrap_or_default();
            let metadata::ReportReference::Path(staged_reference) =
                metadata::pbip_report_reference(&staged).0
            else {
                return Err(reject(
                    Code::StagedContentInvalid,
                    "The staged .pbip content would lose its report reference.",
                ));
            };
            // The reference must still be safe on its own terms, and must
            // still be the very reference the project resolved: this crate
            // edits one boolean, never where the project points.
            if crate::reference::resolve(&[], &staged_reference).is_err() {
                return Err(reject(
                    Code::UnsafeWriteTarget,
                    "The staged .pbip content holds a report reference that does not resolve safely inside the project.",
                ));
            }
            let current = metadata::parse_object(&self.read_managed(path)?).unwrap_or_default();
            if metadata::pbip_report_reference(&current).0
                != metadata::ReportReference::Path(staged_reference)
            {
                return Err(reject(
                    Code::StagedContentInvalid,
                    "The staged .pbip content would change which report the project opens.",
                ));
            }
        } else {
            diagnostics.push(Diagnostic::new(
                Severity::Info,
                Code::UnsafeWriteTarget,
                "A non-.pbip destination was staged through test-support APIs.",
                Some(self.display(path)),
            ));
        }
        Ok(())
    }

    /// Observes the tree. `ignore` names only entries this transaction
    /// created itself; nothing else is ever skipped.
    fn capture_with(&self, ignore: &BTreeSet<String>) -> Result<ProjectSnapshot, SafeWriteError> {
        self.session.verify_root()?;
        capture_snapshot(
            &self.session.root,
            &self.session.root_path,
            &self.session.categories,
            ignore,
        )
    }

    /// Reads a managed file and confirms it still matches the snapshot.
    fn read_managed(&self, path: &str) -> Result<Vec<u8>, SafeWriteError> {
        let (parent, name) = self.resolve(path)?;
        let (bytes, _) = self.read_current(&parent, &name, path)?;
        let recorded = self
            .session
            .snapshot
            .entry(path)
            .and_then(|entry| entry.content_sha256.clone());
        if recorded.as_deref() != Some(snapshot::sha256_hex(&bytes).as_str()) {
            return Err(SafeWriteError::single(
                SafeWriteErrorKind::Conflict,
                Code::ExternalFileChanged,
                "The file changed outside this session since it was opened; reopen the project before editing.",
                Some(self.display(path)),
            ));
        }
        Ok(bytes)
    }

    /// Reads a file's current bytes, bounded, checking the read was stable.
    fn read_current(
        &self,
        parent: &Dir,
        name: &CString,
        path: &str,
    ) -> Result<(Vec<u8>, FileStat), SafeWriteError> {
        let describe = |error: FsError, kind: SafeWriteErrorKind| {
            fs_error(kind, error, "The file", self.display(path))
        };
        let (mut handle, before) = parent
            .open_regular(name)
            .map_err(|error| describe(error, SafeWriteErrorKind::Conflict))?;
        if before.size > MAX_MANAGED_FILE_BYTES {
            return Err(describe(FsError::TooLarge, SafeWriteErrorKind::Unsupported));
        }
        let mut bytes = Vec::with_capacity(before.size as usize);
        std::io::Read::take(&mut handle, MAX_MANAGED_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| describe(FsError::Io, SafeWriteErrorKind::WriteFailed))?;
        if bytes.len() as u64 > MAX_MANAGED_FILE_BYTES {
            return Err(describe(FsError::TooLarge, SafeWriteErrorKind::Unsupported));
        }
        let after = fs_linux::stat_open_file(&handle)
            .map_err(|error| describe(error, SafeWriteErrorKind::WriteFailed))?;
        if !after.same_file(&before) || bytes.len() as u64 != before.size {
            return Err(describe(FsError::Unstable, SafeWriteErrorKind::Conflict));
        }
        Ok((bytes, before))
    }

    /// Opens the parent directory of a relative path, never following
    /// symlinks.
    fn resolve(&self, path: &str) -> Result<(Dir, CString), SafeWriteError> {
        let mut segments: Vec<&str> = path.split('/').collect();
        let file = segments.pop().unwrap_or_default();
        let mut dir = self.session.root.try_clone().map_err(|error| {
            fs_error(
                SafeWriteErrorKind::WriteFailed,
                error,
                "The project folder",
                self.display(path),
            )
        })?;
        for segment in segments {
            let name = segment_name(segment.as_bytes()).map_err(|error| {
                fs_error(
                    SafeWriteErrorKind::Unsupported,
                    error,
                    "A folder in the destination path",
                    self.display(path),
                )
            })?;
            dir = dir.open_subdir(&name).map_err(|error| {
                fs_error(
                    SafeWriteErrorKind::Unsupported,
                    error,
                    "A folder in the destination path",
                    self.display(path),
                )
            })?;
        }
        let name = segment_name(file.as_bytes()).map_err(|error| {
            fs_error(
                SafeWriteErrorKind::Unsupported,
                error,
                "The destination file name",
                self.display(path),
            )
        })?;
        Ok((dir, name))
    }

    fn display(&self, path: &str) -> String {
        self.session
            .root_path
            .join(path)
            .to_string_lossy()
            .into_owned()
    }
}

impl std::fmt::Debug for ProjectTransaction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectTransaction")
            .field("project_file", &self.session.project_file)
            .field("state", &self.session.state)
            .field("staged", &self.staged.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// One file to replace, with everything needed to write and undo it.
struct PlannedWrite {
    path: String,
    parent: Dir,
    name: CString,
    /// The target's state when the plan was made; the replacement must
    /// reproduce its mode and ownership.
    stat: FileStat,
    expected: SnapshotEntry,
    new_bytes: Vec<u8>,
    new_sha256: String,
    original_bytes: Vec<u8>,
    original_sha256: String,
}

/// A file this save actually replaced, with the state of the file it
/// installed, so rollback can tell its own write from someone else's.
struct Replaced {
    path: String,
    /// The file this save wrote, as observed on its own descriptor before it
    /// was renamed into place.
    installed: FileStat,
}

/// Confirms a completed save changed exactly the staged targets and nothing
/// else, while the journal is still available to undo it.
fn expected_changes_only(
    before: &ProjectSnapshot,
    after: &ProjectSnapshot,
    plan: &[PlannedWrite],
    replaced: &[Replaced],
    owned: &BTreeSet<String>,
) -> Result<(), SafeWriteError> {
    // A staged target counts as expected only when the file now at that path
    // is precisely the file this save wrote: the staged bytes, the identity
    // of the descriptor they were written on, and the mode, ownership, and
    // link count the target had before. A same-byte replacement, a chmod, or
    // a chown by somebody else after the rename all fail this.
    let as_written = |path: &str| -> bool {
        let (Some(write), Some(installed), Some(entry)) = (
            plan.iter().find(|write| write.path == path),
            replaced.iter().find(|write| write.path == path),
            after.entry(path),
        ) else {
            return false;
        };
        entry.content_sha256.as_deref() == Some(write.new_sha256.as_str())
            && entry.identity.inode == installed.installed.ino
            && entry.identity.device == installed.installed.dev
            && entry.mode == write.stat.mode
            && entry.uid == write.stat.uid
            && entry.gid == write.stat.gid
            && entry.link_count == write.stat.nlink
            && write
                .parent
                .open_regular(&write.name)
                .is_ok_and(|(handle, stat)| {
                    stat.ino == installed.installed.ino
                        && stat.dev == installed.installed.dev
                        && matches!(fs_linux::has_extended_attributes(&handle), Ok(false))
                })
    };

    let mut unexpected = Vec::new();
    for change in &before.diff(after, owned).changes {
        // A replacement is written to a new file and renamed into place, so
        // a staged target legitimately shows a new inode and new timestamps.
        let expected = matches!(
            change.change,
            ChangeKind::ContentModified | ChangeKind::Replaced | ChangeKind::MetadataChanged
        ) && as_written(&change.path);
        if !expected {
            unexpected.push(change.clone());
        }
    }
    for write in plan {
        if !as_written(&write.path) {
            unexpected.push(EntryChange {
                path: write.path.clone(),
                change: ChangeKind::ContentModified,
            });
        }
    }
    if unexpected.is_empty() {
        return Ok(());
    }
    let mut error = conflict_error(&SnapshotDiff {
        changes: unexpected,
    });
    error.kind = SafeWriteErrorKind::ValidationFailed;
    error.diagnostics.insert(
        0,
        Diagnostic::new(
            Severity::Error,
            Code::PostSaveValidationFailed,
            "After saving, the project held changes beyond the staged files.",
            Some(after.root.clone()),
        ),
    );
    Err(error)
}

/// Returning the session to `Clean` when a transaction is dropped: the
/// staged payload goes with it, so no edits are retained.
impl Drop for ProjectTransaction<'_> {
    fn drop(&mut self) {
        if self.session.state == ProjectState::Dirty {
            self.session.state = ProjectState::Clean;
        }
    }
}

/// On-disk record of a save in progress, holding the original bytes of every
/// target so a failure can be undone and an interrupted save can be seen.
struct Journal {
    project_root: String,
    directory_display: String,
    dir: Dir,
    originals: Dir,
    targets: Vec<JournalTarget>,
    retained: bool,
}

struct JournalTarget {
    path: String,
    original_file: String,
    original_sha256: String,
    new_sha256: String,
    replaced: bool,
}

impl Journal {
    fn create(root: &Dir, root_path: &Path) -> Result<Journal, SafeWriteError> {
        let display = root_path.join(JOURNAL_DIR).to_string_lossy().into_owned();
        let name = segment_name(JOURNAL_DIR.as_bytes()).map_err(|error| {
            fs_error(
                SafeWriteErrorKind::WriteFailed,
                error,
                "The recovery folder",
                display.clone(),
            )
        })?;
        root.create_dir_exclusive(&name, 0o700).map_err(|error| {
            if error == FsError::AlreadyExists {
                SafeWriteError::single(
                    SafeWriteErrorKind::RecoveryPending,
                    Code::RecoveryRequired,
                    "Recovery data from an earlier save is already present; resolve it before saving.",
                    Some(display.clone()),
                )
            } else {
                fs_error(
                    SafeWriteErrorKind::WriteFailed,
                    error,
                    "The recovery folder",
                    display.clone(),
                )
            }
        })?;
        let dir = root.open_subdir(&name).map_err(|error| {
            fs_error(
                SafeWriteErrorKind::WriteFailed,
                error,
                "The recovery folder",
                display.clone(),
            )
        })?;
        let originals_name = segment_name(ORIGINALS_DIR.as_bytes()).map_err(|error| {
            fs_error(
                SafeWriteErrorKind::WriteFailed,
                error,
                "The recovery folder",
                display.clone(),
            )
        })?;
        dir.create_dir_exclusive(&originals_name, 0o700)
            .and_then(|()| dir.open_subdir(&originals_name))
            .map_err(|error| {
                fs_error(
                    SafeWriteErrorKind::WriteFailed,
                    error,
                    "The recovery originals folder",
                    display.clone(),
                )
            })
            .map(|originals| Journal {
                project_root: root_path.to_string_lossy().into_owned(),
                directory_display: display,
                dir,
                originals,
                targets: Vec::new(),
                retained: false,
            })
    }

    /// Copies every target's current bytes into the journal and records the
    /// plan, flushed, before anything is replaced.
    fn stage_originals(&mut self, root: &Dir, plan: &[PlannedWrite]) -> Result<(), SafeWriteError> {
        for (index, write) in plan.iter().enumerate() {
            let file_name = format!("{index:04}");
            let c_name = segment_name(file_name.as_bytes())
                .map_err(|error| self.failure(error, "A staged original name", &write.path))?;
            let mut file = self
                .originals
                .create_new_file(&c_name, 0o600)
                .map_err(|error| self.failure(error, "A staged original", &write.path))?;
            file.write_all(&write.original_bytes)
                .map_err(|_| self.failure(FsError::Io, "A staged original", &write.path))?;
            fs_linux::sync_file(&file)
                .map_err(|error| self.failure(error, "A staged original", &write.path))?;
            self.targets.push(JournalTarget {
                path: write.path.clone(),
                original_file: format!("{ORIGINALS_DIR}/{file_name}"),
                original_sha256: snapshot::sha256_hex(&write.original_bytes),
                new_sha256: write.new_sha256.clone(),
                replaced: false,
            });
        }
        self.originals
            .sync()
            .map_err(|error| self.failure(error, "The recovery originals folder", ""))?;
        self.write_record("prepared")?;
        root.sync()
            .map_err(|error| self.failure(error, "The project folder", ""))?;
        Ok(())
    }

    fn mark_replacing(&mut self, index: usize) -> Result<(), SafeWriteError> {
        if let Some(target) = self.targets.get_mut(index) {
            target.replaced = true;
        }
        self.write_record("replacing")
    }

    fn write_record(&self, phase: &str) -> Result<(), SafeWriteError> {
        let record = serde_json::json!({
            "format": "biforgeworks-save-journal",
            "version": 1,
            "phase": phase,
            "project_root": self.project_root,
            "targets": self.targets.iter().map(|target| serde_json::json!({
                "path": target.path,
                "original_file": target.original_file,
                "original_sha256": target.original_sha256,
                "new_sha256": target.new_sha256,
                "replaced": target.replaced,
            })).collect::<Vec<_>>(),
        })
        .to_string();

        let name = segment_name(JOURNAL_FILE.as_bytes())
            .map_err(|error| self.failure(error, "The recovery journal", ""))?;
        // Replace the record atomically so an interrupted update cannot
        // leave a half-written journal.
        let temp_name = temp_name();
        let c_temp = segment_name(temp_name.as_bytes())
            .map_err(|error| self.failure(error, "The recovery journal", ""))?;
        let mut file = self
            .dir
            .create_new_file(&c_temp, 0o600)
            .map_err(|error| self.failure(error, "The recovery journal", ""))?;
        file.write_all(record.as_bytes())
            .map_err(|_| self.failure(FsError::Io, "The recovery journal", ""))?;
        fs_linux::sync_file(&file)
            .map_err(|error| self.failure(error, "The recovery journal", ""))?;
        drop(file);
        self.dir
            .rename_entry(&c_temp, &name)
            .map_err(|error| self.failure(error, "The recovery journal", ""))?;
        self.dir
            .sync()
            .map_err(|error| self.failure(error, "The recovery folder", ""))
    }

    fn failure(&self, error: FsError, what: &str, path: &str) -> SafeWriteError {
        let display = if path.is_empty() {
            self.directory_display.clone()
        } else {
            format!("{}/{path}", self.directory_display)
        };
        fs_error(SafeWriteErrorKind::WriteFailed, error, what, display)
    }

    /// Keeps the journal on disk for manual recovery.
    fn retain(&mut self) {
        self.retained = true;
    }

    /// Removes only the artifacts this journal created. Anything unexpected
    /// inside is left alone, and reported.
    fn finish(&mut self, root: &Dir) -> Vec<Diagnostic> {
        if self.retained {
            return Vec::new();
        }
        let mut diagnostics = Vec::new();
        let mut failed = false;
        for target in &self.targets {
            let name = target
                .original_file
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            if let Ok(c_name) = segment_name(name.as_bytes()) {
                failed |= self.originals.unlink_entry(&c_name).is_err();
            }
        }
        for (dir, name) in [
            (&self.dir, JOURNAL_FILE),
            (&self.dir, ORIGINALS_DIR),
            (root, JOURNAL_DIR),
        ] {
            let Ok(c_name) = segment_name(name.as_bytes()) else {
                continue;
            };
            let removed = if name == JOURNAL_FILE {
                dir.unlink_entry(&c_name)
            } else {
                dir.remove_dir(&c_name)
            };
            failed |= removed.is_err();
        }
        if failed {
            diagnostics.push(Diagnostic::new(
                Severity::Warning,
                Code::RecoveryArtifactsRetained,
                "Recovery data could not be fully cleaned up. Nothing unrecognized was deleted; remove the folder by hand once you have checked it.",
                Some(self.directory_display.clone()),
            ));
        }
        let _ = root.sync();
        diagnostics
    }
}

impl Drop for Journal {
    fn drop(&mut self) {
        // A journal dropped without an explicit outcome is retained rather
        // than cleaned up: it is the only record of an in-flight save.
    }
}

/// Same-process exclusion keyed by the root directory's identity, so two
/// different paths to the same project (symlinked or bind-mounted aliases)
/// still exclude each other.
struct SaveLock {
    key: (u64, u64),
}

fn active_saves() -> &'static Mutex<BTreeSet<(u64, u64)>> {
    static ACTIVE: OnceLock<Mutex<BTreeSet<(u64, u64)>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(BTreeSet::new()))
}

impl SaveLock {
    fn acquire(device: u64, inode: u64) -> Option<SaveLock> {
        let key = (device, inode);
        let mut active = active_saves()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !active.insert(key) {
            return None;
        }
        Some(SaveLock { key })
    }
}

impl Drop for SaveLock {
    fn drop(&mut self) {
        active_saves()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.key);
    }
}

fn temp_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!(
        "{TEMP_PREFIX}{}-{}-{nanos}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn injected(kind: SafeWriteErrorKind, stage: &str) -> SafeWriteError {
    SafeWriteError::single(
        kind,
        Code::WriteFailed,
        format!("Injected test failure {stage}."),
        None,
    )
}

fn conflict_error(diff: &SnapshotDiff) -> SafeWriteError {
    let diagnostics = diff
        .changes
        .iter()
        .map(|change| {
            let (code, message) = match change.change {
                ChangeKind::Added => (
                    Code::ExternalFileAdded,
                    "A file was added outside this session.",
                ),
                ChangeKind::Removed => (
                    Code::ExternalFileRemoved,
                    "A file was deleted outside this session.",
                ),
                ChangeKind::ContentModified => (
                    Code::ExternalFileChanged,
                    "A file's contents changed outside this session.",
                ),
                ChangeKind::Replaced => (
                    Code::ExternalFileReplaced,
                    "A file was replaced outside this session.",
                ),
                ChangeKind::TypeChanged => (
                    Code::ExternalFileTypeChanged,
                    "A path changed type outside this session.",
                ),
                ChangeKind::MetadataChanged => (
                    Code::ExternalFileChanged,
                    "A file's permissions, ownership, or link count changed outside this session.",
                ),
                ChangeKind::RootReplaced => (
                    Code::ExternalFileReplaced,
                    "The project folder is no longer the folder this session opened.",
                ),
            };
            Diagnostic::new(
                Severity::Error,
                code,
                message,
                (!change.path.is_empty()).then(|| change.path.clone()),
            )
        })
        .collect();
    SafeWriteError::new(SafeWriteErrorKind::Conflict, diagnostics)
}

fn fs_error(kind: SafeWriteErrorKind, error: FsError, what: &str, path: String) -> SafeWriteError {
    let (code, message) = match error {
        FsError::NotFound => (Code::ExternalFileRemoved, format!("{what} was not found.")),
        FsError::Symlink => (
            Code::SymlinkRejected,
            format!("{what} is a symbolic link, which is never followed."),
        ),
        FsError::NotDirectory => (Code::NotADirectory, format!("{what} is not a directory.")),
        FsError::NotRegularFile => (
            Code::NotARegularFile,
            format!("{what} is not a regular file."),
        ),
        FsError::TooLarge => (
            Code::MetadataTooLarge,
            format!("{what} is larger than the supported limit."),
        ),
        FsError::AccessDenied => (
            Code::FileAccessDenied,
            format!("{what} could not be accessed (permission denied)."),
        ),
        FsError::NoAtimeUnavailable => (
            Code::ReadOnlyGuaranteeUnavailable,
            format!("{what} could not be read without updating its access time."),
        ),
        FsError::AlreadyExists => (
            Code::WriteFailed,
            format!("{what} already exists where a new file was required."),
        ),
        FsError::Unstable => (
            Code::ExternalFileChanged,
            format!("{what} changed while it was being read."),
        ),
        FsError::Unsupported => (
            Code::UnsafeWriteTarget,
            format!("{what} would not keep the original file's access metadata."),
        ),
        FsError::InvalidName | FsError::Io => {
            (Code::WriteFailed, format!("{what} could not be written."))
        }
    };
    SafeWriteError::single(kind, code, message, Some(path))
}

fn capture_snapshot(
    root: &Dir,
    root_path: &Path,
    categories: &Categories,
    ignore: &BTreeSet<String>,
) -> Result<ProjectSnapshot, SafeWriteError> {
    let display = root_path.to_string_lossy().into_owned();
    snapshot::capture(root, &display, categories, ignore).map_err(|error| {
        let (code, message, path) = match error {
            SnapshotError::Unreadable { path, .. } => (
                Code::SnapshotIncomplete,
                "An entry in the project could not be read, so preservation could not be verified.",
                Some(path),
            ),
            SnapshotError::Unstable { path } => (
                Code::ExternalFileChanged,
                "An entry changed while the project was being recorded.",
                Some(path),
            ),
            SnapshotError::TooManyEntries => (
                Code::SnapshotTooLarge,
                "The project contains more entries than this crate will record.",
                None,
            ),
            SnapshotError::TooDeep { path } => (
                Code::SnapshotTooLarge,
                "The project is nested more deeply than this crate will record.",
                Some(path),
            ),
            SnapshotError::TooLarge { path } => (
                Code::SnapshotTooLarge,
                "The project holds more bytes than this crate will record.",
                Some(path),
            ),
            SnapshotError::UnsupportedName { path } => (
                Code::PathNotUtf8,
                "The project contains a name that is not valid UTF-8.",
                Some(path),
            ),
        };
        let path = path.map(|relative| {
            if relative.is_empty() {
                display.clone()
            } else {
                root_path.join(relative).to_string_lossy().into_owned()
            }
        });
        SafeWriteError::single(SafeWriteErrorKind::SnapshotFailed, code, message, path)
    })
}

/// Looks for recovery data left by an interrupted save. Artifacts are only
/// reported, never removed or replayed.
fn detect_recovery(
    root: &Dir,
    root_path: &Path,
) -> Result<Option<RecoveryArtifacts>, SafeWriteError> {
    let display = root_path.join(JOURNAL_DIR).to_string_lossy().into_owned();
    let name = match segment_name(JOURNAL_DIR.as_bytes()) {
        Ok(name) => name,
        Err(_) => return Ok(None),
    };
    let kind = root.entry_kind(&name).map_err(|error| {
        fs_error(
            SafeWriteErrorKind::OpenFailed,
            error,
            "The recovery folder",
            display.clone(),
        )
    })?;
    if kind == EntryKind::Missing {
        return Ok(None);
    }

    let mut artifacts = RecoveryArtifacts {
        journal_directory: display.clone(),
        originals_directory: format!("{display}/{ORIGINALS_DIR}"),
        targets: Vec::new(),
        journal_unreadable: true,
    };
    if kind != EntryKind::Directory {
        return Ok(Some(artifacts));
    }
    let Ok(dir) = root.open_subdir(&name) else {
        return Ok(Some(artifacts));
    };
    let Ok(journal_name) = segment_name(JOURNAL_FILE.as_bytes()) else {
        return Ok(Some(artifacts));
    };
    let Ok(bytes) = dir.read_file(&journal_name, MAX_MANAGED_FILE_BYTES) else {
        return Ok(Some(artifacts));
    };
    let Ok(record) = metadata::parse_object(&bytes) else {
        return Ok(Some(artifacts));
    };
    if record.get("format").and_then(|v| v.as_str()) != Some("biforgeworks-save-journal") {
        return Ok(Some(artifacts));
    }
    artifacts.journal_unreadable = false;
    if let Some(targets) = record.get("targets").and_then(|v| v.as_array()) {
        for target in targets {
            let path = target
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            artifacts.targets.push(RecoveryTarget {
                path: path.to_owned(),
                original_file: target
                    .get("original_file")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                original_sha256: target
                    .get("original_sha256")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                replaced: target
                    .get("replaced")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            });
        }
    }
    Ok(Some(artifacts))
}

/// Decides which relative paths are managed and which are preserved,
/// entirely from the discovery result.
fn classify(summary: &PowerBiProjectSummary, managed_file: &str, root_path: &Path) -> Categories {
    let mut preserved = BTreeSet::new();
    let relative = |absolute: &str| -> Option<String> {
        Path::new(absolute)
            .strip_prefix(root_path)
            .ok()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .filter(|path| !path.is_empty())
    };

    if let Some(report) = summary.report.path.as_deref().and_then(relative) {
        for marker in [
            "definition.pbir",
            "report.json",
            "definition/report.json",
            "definition/version.json",
        ] {
            preserved.insert(format!("{report}/{marker}"));
        }
        preserved.insert(report);
    }
    if let Some(model) = summary.semantic_model.path.as_deref().and_then(relative) {
        for marker in ["definition.pbism", "model.bim", "definition/model.tmdl"] {
            preserved.insert(format!("{model}/{marker}"));
        }
        preserved.insert(model);
    }
    preserved.remove(managed_file);

    Categories {
        managed: BTreeSet::from([managed_file.to_owned()]),
        preserved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    use std::sync::atomic::AtomicUsize;

    const PBIP_SCHEMA: &str =
        "https://developer.microsoft.com/json-schemas/fabric/pbip/pbipProperties/1.0.0/schema.json";
    const PBIR_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json";
    const PBISM_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/semanticModel/definitionProperties/1.0.0/schema.json";
    const VERSION_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/versionMetadata/1.0.0/schema.json";

    /// A named case with the external change it makes.
    type Case = (&'static str, fn(&Path));
    /// A named case with the change it makes and the code it must produce.
    type ConflictCase = (&'static str, fn(&Path), Code);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "biforgeworks-safe-writes-{tag}-{}-{}-{nanos}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create temp dir");
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, bytes: impl AsRef<[u8]>) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parents");
        }
        fs::write(path, bytes).expect("write file");
    }

    /// A complete PBIR/TMDL project whose `.pbip` carries CRLF, a BOM, an
    /// unknown property, and `settings.enableAutoRecovery`.
    fn project(root: &Path, auto_recovery: bool) -> PathBuf {
        let pbip = root.join("Sales.pbip");
        write(&pbip, pbip_bytes(auto_recovery));
        write(
            &root.join("Sales.Report/definition.pbir"),
            format!(
                r#"{{"$schema": "{PBIR_SCHEMA}", "version": "4.0", "datasetReference": {{"byPath": {{"path": "../Sales.SemanticModel"}}}}}}"#
            ),
        );
        write(
            &root.join("Sales.Report/definition/version.json"),
            format!(r#"{{"$schema": "{VERSION_SCHEMA}", "version": "2.0.0"}}"#),
        );
        write(&root.join("Sales.Report/definition/report.json"), "{}");
        write(
            &root.join("Sales.SemanticModel/definition.pbism"),
            format!(r#"{{"$schema": "{PBISM_SCHEMA}", "version": "4.0", "settings": {{}}}}"#),
        );
        write(
            &root.join("Sales.SemanticModel/definition/model.tmdl"),
            "model Model\n\tculture: en-US\n",
        );
        write(&root.join("notes/unknown.txt"), "kept verbatim\r\n");
        pbip
    }

    fn pbip_bytes(auto_recovery: bool) -> Vec<u8> {
        let text = format!(
            "\u{FEFF}{{\r\n  \"$schema\": \"{PBIP_SCHEMA}\",\r\n  \"version\": \"1.0\",\r\n  \"artifacts\": [\r\n    {{\r\n      \"report\": {{\r\n        \"path\": \"Sales.Report\"\r\n      }}\r\n    }}\r\n  ],\r\n  \"unknownProperty\": {{\"kept\": [1, 2, 3]}},\r\n  \"settings\": {{\r\n    \"enableAutoRecovery\": {auto_recovery}\r\n  }}\r\n}}\r\n"
        );
        text.into_bytes()
    }

    /// Every path under a tree with its bytes and the metadata a save must
    /// not disturb.
    fn tree(root: &Path) -> BTreeMap<String, (Vec<u8>, u32, u64)> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).expect("read dir") {
                let entry = entry.expect("entry");
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .expect("relative")
                    .to_string_lossy()
                    .into_owned();
                let meta = fs::symlink_metadata(&path).expect("lstat");
                if meta.is_dir() {
                    out.insert(relative, (Vec::new(), meta.mode(), meta.ino()));
                    stack.push(path);
                } else if meta.is_file() {
                    out.insert(
                        relative,
                        (fs::read(&path).expect("read"), meta.mode(), meta.ino()),
                    );
                } else {
                    out.insert(relative, (Vec::new(), meta.mode(), meta.ino()));
                }
            }
        }
        out
    }

    /// Contents and permissions only. After a rollback a restored file
    /// legitimately has a new inode, because restoring also goes through a
    /// temporary file and a rename.
    fn contents(root: &Path) -> BTreeMap<String, (Vec<u8>, u32)> {
        tree(root)
            .into_iter()
            .map(|(path, (bytes, mode, _))| (path, (bytes, mode)))
            .collect()
    }

    fn open(pbip: &Path) -> ProjectSession {
        ProjectSession::open(pbip).expect("session opens")
    }

    fn auto_recovery_value(pbip: &Path) -> bool {
        let bytes = fs::read(pbip).expect("read pbip");
        let document = metadata::parse_object(&bytes).expect("valid json");
        document["settings"]["enableAutoRecovery"]
            .as_bool()
            .expect("boolean")
    }

    #[test]
    fn open_records_categories_for_managed_preserved_and_unknown_files() {
        let temp = TempDir::new("categories");
        let pbip = project(temp.path(), true);
        let session = open(&pbip);

        assert_eq!(session.state(), ProjectState::Clean);
        assert!(session.pending_recovery().is_none());
        let snapshot = session.snapshot();
        assert_eq!(
            snapshot.entry("Sales.pbip").unwrap().category,
            FileCategory::Managed
        );
        assert_eq!(
            snapshot
                .entry("Sales.Report/definition.pbir")
                .unwrap()
                .category,
            FileCategory::Preserved
        );
        assert_eq!(
            snapshot
                .entry("Sales.SemanticModel/definition/model.tmdl")
                .unwrap()
                .category,
            FileCategory::Preserved
        );
        assert_eq!(
            snapshot.entry("notes/unknown.txt").unwrap().category,
            FileCategory::Unknown
        );
        // Directories and their contents are recorded, hashes for files only.
        assert_eq!(
            snapshot.entry("notes").unwrap().entry_type,
            EntryType::Directory
        );
        assert!(snapshot.entry("notes").unwrap().content_sha256.is_none());
        assert!(snapshot
            .entry("notes/unknown.txt")
            .unwrap()
            .content_sha256
            .is_some());
    }

    #[test]
    fn snapshot_tracks_symlinks_and_special_files_without_following_them() {
        let temp = TempDir::new("opaque");
        let pbip = project(temp.path(), true);
        symlink("/etc/hostname", temp.path().join("link")).expect("symlink");
        let session = open(&pbip);

        let link = session.snapshot().entry("link").expect("symlink recorded");
        assert_eq!(link.entry_type, EntryType::Symlink);
        assert_eq!(link.content_sha256, None);
        assert_eq!(link.category, FileCategory::Unknown);
    }

    #[test]
    fn controlled_edit_changes_only_the_flag_and_preserves_everything_else() {
        let temp = TempDir::new("edit");
        let pbip = project(temp.path(), true);
        let before = tree(temp.path());
        let mut session = open(&pbip);

        let mut transaction = session.begin_transaction().expect("transaction");
        assert_eq!(
            transaction.stage_auto_recovery(false).unwrap(),
            StageOutcome::Changed
        );
        assert_eq!(transaction.staged().len(), 1);
        assert_eq!(transaction.staged()[0].path, "Sales.pbip");
        let receipt = transaction.commit().expect("commit succeeds");

        assert_eq!(receipt.changed_files, vec!["Sales.pbip".to_owned()]);
        assert!(!receipt.unchanged);
        assert_eq!(session.state(), ProjectState::Clean);
        assert!(!auto_recovery_value(&pbip));

        let after = tree(temp.path());
        // Only the .pbip changed, and only in the one token.
        for (path, value) in &before {
            let (bytes, mode, ino) = after.get(path).expect("entry survives");
            assert_eq!(mode, &value.1, "{path} mode");
            if path == "Sales.pbip" {
                assert_eq!(bytes, &pbip_bytes(false));
                assert_eq!(
                    String::from_utf8_lossy(&value.0).replace("true", "false"),
                    String::from_utf8_lossy(bytes)
                );
            } else {
                assert_eq!(bytes, &value.0, "{path} content");
                assert_eq!(ino, &value.2, "{path} identity");
            }
        }
        assert_eq!(before.len(), after.len(), "no files added or removed");
        // No journal or temporary files are left behind.
        assert!(!temp.path().join(JOURNAL_DIR).exists());
        // The receipt carries a fresh discovery and snapshot.
        assert_eq!(receipt.summary.report.format, ComponentFormat::Pbir);
        assert_eq!(
            receipt.snapshot.entry("Sales.pbip").unwrap().content_sha256,
            Some(snapshot::sha256_hex(&pbip_bytes(false)))
        );
    }

    #[test]
    fn no_op_commit_writes_nothing_at_all() {
        let temp = TempDir::new("noop");
        let pbip = project(temp.path(), true);
        let before = tree(temp.path());
        let times: Vec<_> = before
            .keys()
            .map(|path| {
                let meta = fs::metadata(temp.path().join(path)).expect("stat");
                (meta.mtime(), meta.mtime_nsec(), meta.ctime())
            })
            .collect();

        let mut session = open(&pbip);
        let mut transaction = session.begin_transaction().expect("transaction");
        assert_eq!(
            transaction.stage_auto_recovery(true).unwrap(),
            StageOutcome::AlreadyMatches
        );
        assert!(transaction.staged().is_empty());
        let receipt = transaction.commit().expect("commit");

        assert!(receipt.unchanged);
        assert!(receipt.changed_files.is_empty());
        assert_eq!(tree(temp.path()), before);
        for (index, path) in before.keys().enumerate() {
            let meta = fs::metadata(temp.path().join(path)).expect("stat");
            assert_eq!(
                (meta.mtime(), meta.mtime_nsec(), meta.ctime()),
                times[index],
                "{path} timestamps"
            );
        }
        assert!(!temp.path().join(JOURNAL_DIR).exists());
    }

    #[test]
    fn staging_requires_an_existing_boolean() {
        let temp = TempDir::new("absent-flag");
        let pbip = project(temp.path(), true);
        for content in [
            r#"{"version": "1.0", "artifacts": [{"report": {"path": "Sales.Report"}}]}"#,
            r#"{"version": "1.0", "artifacts": [{"report": {"path": "Sales.Report"}}], "settings": {}}"#,
            r#"{"version": "1.0", "artifacts": [{"report": {"path": "Sales.Report"}}], "settings": {"enableAutoRecovery": "true"}}"#,
            r#"{"version": "1.0", "artifacts": [{"report": {"path": "Sales.Report"}}], "settings": null}"#,
        ] {
            write(&pbip, content);
            let mut session = open(&pbip);
            let mut transaction = session.begin_transaction().expect("transaction");
            let error = transaction.stage_auto_recovery(false).unwrap_err();
            assert_eq!(error.kind, SafeWriteErrorKind::StageRejected);
            assert_eq!(
                error.diagnostics[0].code,
                Code::AutoRecoverySettingUnavailable
            );
        }
    }

    #[test]
    fn external_changes_to_any_category_block_the_save() {
        let cases: &[ConflictCase] = &[
            (
                "unknown file edited",
                |root: &Path| write(&root.join("notes/unknown.txt"), "changed"),
                Code::ExternalFileChanged,
            ),
            (
                "preserved file edited",
                |root: &Path| write(&root.join("Sales.Report/definition/report.json"), "{ }"),
                Code::ExternalFileChanged,
            ),
            (
                "preserved file deleted",
                |root: &Path| {
                    fs::remove_file(root.join("Sales.SemanticModel/definition/model.tmdl"))
                        .expect("remove")
                },
                Code::ExternalFileRemoved,
            ),
            (
                "file added",
                |root: &Path| write(&root.join("notes/new.txt"), "new"),
                Code::ExternalFileAdded,
            ),
            (
                // Replaced atomically from a sibling file, so the live inode
                // is certainly a different one (unlink-then-recreate could
                // reuse the same inode number).
                "file replaced with identical bytes",
                |root: &Path| {
                    let path = root.join("notes/unknown.txt");
                    let bytes = fs::read(&path).expect("read");
                    let replacement = root.join("notes/replacement.tmp");
                    write(&replacement, bytes);
                    fs::rename(&replacement, &path).expect("rename");
                },
                Code::ExternalFileReplaced,
            ),
            (
                "file rewritten with identical bytes",
                |root: &Path| {
                    let path = root.join("notes/unknown.txt");
                    let bytes = fs::read(&path).expect("read");
                    // Same inode, same bytes: only the timestamps move.
                    write(&path, bytes);
                },
                Code::ExternalFileChanged,
            ),
            (
                "file becomes a directory",
                |root: &Path| {
                    fs::remove_file(root.join("notes/unknown.txt")).expect("remove");
                    fs::create_dir(root.join("notes/unknown.txt")).expect("mkdir");
                },
                Code::ExternalFileTypeChanged,
            ),
            (
                "permissions changed",
                |root: &Path| {
                    fs::set_permissions(
                        root.join("notes/unknown.txt"),
                        fs::Permissions::from_mode(0o600),
                    )
                    .expect("chmod")
                },
                Code::ExternalFileChanged,
            ),
        ];

        for (name, mutate, expected) in cases {
            let temp = TempDir::new("conflict");
            let pbip = project(temp.path(), true);
            let mut session = open(&pbip);
            let mut transaction = session.begin_transaction().expect("transaction");
            transaction.stage_auto_recovery(false).expect("staged");

            mutate(temp.path());
            let before = tree(temp.path());
            let error = transaction.commit().unwrap_err();

            assert_eq!(error.kind, SafeWriteErrorKind::Conflict, "{name}");
            assert!(
                error.diagnostics.iter().any(|d| d.code == *expected),
                "{name}: {:?}",
                error.diagnostics
            );
            assert_eq!(session.state(), ProjectState::Conflict, "{name}");
            assert_eq!(tree(temp.path()), before, "{name}: nothing was written");
            assert!(auto_recovery_value(&pbip), "{name}: the flag is unchanged");
        }
    }

    #[test]
    fn managed_file_edited_externally_is_caught_at_staging_time() {
        let temp = TempDir::new("managed-change");
        let pbip = project(temp.path(), true);
        let mut session = open(&pbip);
        let mut transaction = session.begin_transaction().expect("transaction");

        write(&pbip, pbip_bytes(false));
        let error = transaction.stage_auto_recovery(true).unwrap_err();
        assert_eq!(error.kind, SafeWriteErrorKind::Conflict);
        assert_eq!(error.diagnostics[0].code, Code::ExternalFileChanged);
    }

    #[test]
    fn files_carrying_extended_attributes_are_refused() {
        let temp = TempDir::new("xattr-target");
        let pbip = project(temp.path(), true);
        let c_path = CString::new(pbip.as_os_str().as_bytes()).expect("path");
        let name = c"user.biforgeworks.test";
        // SAFETY: both pointers are valid NUL-terminated strings and the
        // value buffer is the length passed.
        let set = unsafe {
            libc::setxattr(
                c_path.as_ptr(),
                name.as_ptr(),
                b"x".as_ptr().cast::<libc::c_void>(),
                1,
                0,
            )
        };
        if set != 0 {
            eprintln!("skipping: this filesystem does not support user extended attributes");
            return;
        }

        let mut session = open(&pbip);
        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");
        let error = transaction.commit().unwrap_err();

        assert_eq!(error.kind, SafeWriteErrorKind::Unsupported);
        assert_eq!(error.diagnostics[0].code, Code::UnsafeWriteTarget);
        assert!(auto_recovery_value(&pbip), "the file was left alone");
        assert!(!temp.path().join(JOURNAL_DIR).exists());
    }

    #[test]
    fn a_replacement_that_would_inherit_a_default_acl_is_refused() {
        let temp = TempDir::new("default-acl");
        let pbip = project(temp.path(), true);

        // A POSIX default ACL on the project folder is inherited by every
        // file created in it, including our replacement — but not by the
        // existing target. Layout: version, then 8-byte entries of
        // {tag, permissions, id}, in tag order. The named-user entry is what
        // makes the inherited ACL unrepresentable as mode bits, so the
        // kernel stores it as an extended attribute.
        const USER_OBJ: u16 = 0x01;
        const USER: u16 = 0x02;
        const GROUP_OBJ: u16 = 0x04;
        const MASK: u16 = 0x10;
        const OTHER: u16 = 0x20;
        let mut acl: Vec<u8> = 2u32.to_ne_bytes().to_vec();
        for (tag, permissions, id) in [
            (USER_OBJ, 7u16, u32::MAX),
            (USER, 4, 0),
            (GROUP_OBJ, 5, u32::MAX),
            (MASK, 7, u32::MAX),
            (OTHER, 5, u32::MAX),
        ] {
            acl.extend_from_slice(&tag.to_ne_bytes());
            acl.extend_from_slice(&permissions.to_ne_bytes());
            acl.extend_from_slice(&id.to_ne_bytes());
        }
        let c_dir = CString::new(temp.path().as_os_str().as_bytes()).expect("path");
        // SAFETY: both pointers are valid, and `acl.len()` describes the
        // value buffer.
        let set = unsafe {
            libc::setxattr(
                c_dir.as_ptr(),
                c"system.posix_acl_default".as_ptr(),
                acl.as_ptr().cast::<libc::c_void>(),
                acl.len(),
                0,
            )
        };
        if set != 0 {
            eprintln!("skipping: this filesystem does not support POSIX ACLs");
            return;
        }

        let mut session = open(&pbip);
        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");
        let error = transaction.commit().unwrap_err();

        assert_eq!(error.kind, SafeWriteErrorKind::WriteFailed, "{error:?}");
        assert_eq!(
            error.diagnostics[0].code,
            Code::UnsafeWriteTarget,
            "{error:?}"
        );
        assert!(auto_recovery_value(&pbip), "the target was left alone");
        assert!(!temp.path().join(JOURNAL_DIR).exists());
    }

    #[test]
    fn hard_linked_targets_are_refused() {
        let temp = TempDir::new("unsafe-target");
        let pbip = project(temp.path(), true);
        fs::hard_link(&pbip, temp.path().join("alias.pbip")).expect("hard link");

        let mut session = open(&pbip);
        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");
        let error = transaction.commit().unwrap_err();

        assert_eq!(error.kind, SafeWriteErrorKind::Unsupported);
        assert_eq!(error.diagnostics[0].code, Code::UnsafeWriteTarget);
        assert!(auto_recovery_value(&pbip));
        assert!(!temp.path().join(JOURNAL_DIR).exists());
    }

    #[test]
    fn preserved_and_unknown_destinations_cannot_be_staged() {
        let temp = TempDir::new("destinations");
        let pbip = project(temp.path(), true);
        let mut session = open(&pbip);
        let mut transaction = session.begin_transaction().expect("transaction");

        transaction
            .stage_replacement_for_tests("Sales.Report/definition.pbir", b"{}".to_vec())
            .expect("staging itself is allowed");
        let error = transaction.validate().unwrap_err();
        assert_eq!(error.kind, SafeWriteErrorKind::ValidationFailed);
        assert_eq!(error.diagnostics[0].code, Code::UnsafeWriteTarget);
    }

    #[test]
    fn staged_pbip_content_must_stay_valid_and_keep_its_reference() {
        let temp = TempDir::new("staged-content");
        let pbip = project(temp.path(), true);
        let mut session = open(&pbip);

        for (bytes, code) in [
            (b"not json".to_vec(), Code::StagedContentInvalid),
            (
                br#"{"version": "1.0", "artifacts": []}"#.to_vec(),
                Code::StagedContentInvalid,
            ),
            (
                br#"{"version": "1.0", "artifacts": [{"report": {"path": "../Outside.Report"}}]}"#
                    .to_vec(),
                Code::UnsafeWriteTarget,
            ),
            (
                br#"{"version": "1.0", "artifacts": [{"report": {"path": "Other.Report"}}]}"#
                    .to_vec(),
                Code::StagedContentInvalid,
            ),
        ] {
            let mut transaction = session.begin_transaction().expect("transaction");
            transaction
                .stage_replacement_for_tests("Sales.pbip", bytes)
                .expect("staged");
            let error = transaction.validate().unwrap_err();
            assert_eq!(error.kind, SafeWriteErrorKind::ValidationFailed);
            assert_eq!(error.diagnostics[0].code, code);
        }
    }

    #[test]
    fn failures_before_any_replacement_leave_the_project_untouched() {
        for point in [
            FailurePoint::AfterStaging,
            FailurePoint::AfterOriginalsStaged,
            FailurePoint::DuringTempWrite,
            FailurePoint::BeforeReplace,
        ] {
            let temp = TempDir::new("early-failure");
            let pbip = project(temp.path(), true);
            let before = tree(temp.path());
            let mut session = open(&pbip);

            let mut transaction = session.begin_transaction().expect("transaction");
            transaction.stage_auto_recovery(false).expect("staged");
            transaction.inject_failure(point);
            let error = transaction.commit().unwrap_err();

            assert_eq!(error.kind, SafeWriteErrorKind::WriteFailed, "{point:?}");
            assert_eq!(session.state(), ProjectState::Error, "{point:?}");
            assert!(auto_recovery_value(&pbip), "{point:?}");
            assert_eq!(tree(temp.path()), before, "{point:?}: nothing changed");
            assert!(
                !temp.path().join(JOURNAL_DIR).exists(),
                "{point:?}: journal cleaned up"
            );
        }
    }

    #[test]
    fn failure_after_the_first_of_several_replacements_rolls_everything_back() {
        let temp = TempDir::new("multi-rollback");
        let pbip = project(temp.path(), true);
        let before = contents(temp.path());
        let mut session = open(&pbip);

        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");
        transaction
            .stage_replacement_for_tests("notes/unknown.txt", b"rewritten".to_vec())
            .expect("staged");
        assert_eq!(transaction.staged().len(), 2);
        transaction.inject_failure(FailurePoint::AfterFirstReplace);
        let error = transaction.commit().unwrap_err();

        assert_eq!(error.kind, SafeWriteErrorKind::RolledBack);
        assert!(error
            .diagnostics
            .iter()
            .any(|d| d.code == Code::SaveRolledBack));
        assert_eq!(session.state(), ProjectState::Error);
        assert_eq!(
            contents(temp.path()),
            before,
            "every file is back to its original"
        );
        assert!(!temp.path().join(JOURNAL_DIR).exists());
    }

    #[test]
    fn post_save_validation_failure_rolls_back() {
        let temp = TempDir::new("post-validate");
        let pbip = project(temp.path(), true);
        let before = contents(temp.path());
        let mut session = open(&pbip);

        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");
        transaction.inject_failure(FailurePoint::PostValidation);
        let error = transaction.commit().unwrap_err();

        assert_eq!(error.kind, SafeWriteErrorKind::RolledBack);
        assert_eq!(contents(temp.path()), before);
        assert!(auto_recovery_value(&pbip));
    }

    #[test]
    fn rollback_failure_retains_originals_and_demands_recovery() {
        let temp = TempDir::new("rollback-failure");
        let pbip = project(temp.path(), true);
        let mut session = open(&pbip);

        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");
        transaction
            .stage_replacement_for_tests("notes/unknown.txt", b"rewritten".to_vec())
            .expect("staged");
        // The hook fails post-save validation and then fails the restore.
        transaction.inject_failure(FailurePoint::RollbackRestore);
        let error = transaction.commit().unwrap_err();

        assert_eq!(error.kind, SafeWriteErrorKind::RecoveryRequired);
        assert!(error
            .diagnostics
            .iter()
            .any(|d| d.code == Code::RollbackFailed));
        assert_eq!(session.state(), ProjectState::Error);

        // Originals are retained, and the next session refuses to save.
        let journal = temp.path().join(JOURNAL_DIR);
        assert!(journal.join("journal.json").exists());
        assert!(journal.join("originals").exists());

        let mut reopened = open(&pbip);
        let recovery = reopened.pending_recovery().expect("recovery reported");
        assert!(!recovery.journal_unreadable);
        assert!(!recovery.targets.is_empty());
        assert_eq!(reopened.state(), ProjectState::Error);
        let Err(blocked) = reopened.begin_transaction() else {
            panic!("a session with pending recovery must refuse transactions");
        };
        assert_eq!(blocked.kind, SafeWriteErrorKind::RecoveryPending);
    }

    #[test]
    fn rollback_never_overwrites_a_file_changed_after_the_save_wrote_it() {
        // Each case makes a different kind of external change at exactly the
        // moment rollback is about to restore, so no timing race decides the
        // outcome.
        let cases: &[Case] = &[
            ("different content", |pbip: &Path| {
                fs::write(pbip, b"edited by something else").expect("external write");
            }),
            ("same bytes, new inode", |pbip: &Path| {
                // A writer that replaces rather than rewrites: identical
                // bytes, different file.
                let bytes = fs::read(pbip).expect("read");
                let temp = pbip.with_extension("pbip.other");
                fs::write(&temp, bytes).expect("write");
                fs::rename(&temp, pbip).expect("rename");
            }),
            ("permissions changed", |pbip: &Path| {
                fs::set_permissions(pbip, fs::Permissions::from_mode(0o600)).expect("chmod");
            }),
        ];

        for (name, mutate) in cases {
            let temp = TempDir::new("rollback-external");
            let pbip = project(temp.path(), true);
            let mut session = open(&pbip);

            let mut transaction = session.begin_transaction().expect("transaction");
            transaction.stage_auto_recovery(false).expect("staged");
            transaction.inject_failure(FailurePoint::PostValidation);
            let target = pbip.clone();
            let mutate = *mutate;
            transaction.on_before_rollback(Box::new(move || mutate(&target)));
            let before_rollback_marker = pbip_bytes(false);
            let error = transaction.commit().unwrap_err();

            assert_eq!(error.kind, SafeWriteErrorKind::RecoveryRequired, "{name}");
            assert!(
                error
                    .diagnostics
                    .iter()
                    .any(|d| d.code == Code::RollbackFailed),
                "{name}: {:?}",
                error.diagnostics
            );
            assert_eq!(session.state(), ProjectState::Error, "{name}");

            // The external change survived; the original was not written over
            // it, and the originals are retained for manual recovery.
            let current = fs::read(&pbip).expect("read");
            match *name {
                "different content" => assert_eq!(current, b"edited by something else"),
                _ => assert_eq!(current, before_rollback_marker, "{name}"),
            }
            assert_ne!(current, pbip_bytes(true), "{name}: original not restored");
            assert!(
                temp.path().join(JOURNAL_DIR).join("journal.json").exists(),
                "{name}: recovery data retained"
            );
        }
    }

    #[test]
    fn a_change_landing_right_after_our_rename_is_never_blessed_as_ours() {
        // Each case interferes in the instant after the rename, where the
        // written bytes alone would look correct.
        let cases: &[Case] = &[
            ("permissions changed", |pbip: &Path| {
                fs::set_permissions(pbip, fs::Permissions::from_mode(0o600)).expect("chmod");
            }),
            ("replaced with the same bytes", |pbip: &Path| {
                let bytes = fs::read(pbip).expect("read");
                let temp = pbip.with_extension("pbip.other");
                fs::write(&temp, bytes).expect("write");
                fs::rename(&temp, pbip).expect("rename");
            }),
            ("hard link added", |pbip: &Path| {
                let link = pbip.with_extension("pbip.link");
                fs::hard_link(pbip, &link).expect("hard link");
            }),
        ];

        for (name, mutate) in cases {
            let temp = TempDir::new("after-replace");
            let pbip = project(temp.path(), true);
            let mut session = open(&pbip);

            let mut transaction = session.begin_transaction().expect("transaction");
            transaction.stage_auto_recovery(false).expect("staged");
            let target = pbip.clone();
            let mutate = *mutate;
            transaction.on_after_replace(Box::new(move || mutate(&target)));
            let error = transaction.commit().unwrap_err();

            // The save never reports success, and the interference is never
            // adopted into the refreshed snapshot.
            assert!(
                matches!(
                    error.kind,
                    SafeWriteErrorKind::Conflict
                        | SafeWriteErrorKind::RolledBack
                        | SafeWriteErrorKind::RecoveryRequired
                ),
                "{name}: {error:?}"
            );
            assert_ne!(session.state(), ProjectState::Clean, "{name}");
            assert_eq!(
                session
                    .snapshot()
                    .entry("Sales.pbip")
                    .unwrap()
                    .content_sha256,
                Some(snapshot::sha256_hex(&pbip_bytes(true))),
                "{name}: the session still describes the original file"
            );
        }
    }

    #[test]
    fn rollback_accepts_a_file_already_back_to_its_original_bytes() {
        let temp = TempDir::new("rollback-reverted");
        let pbip = project(temp.path(), true);
        let mut session = open(&pbip);

        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");
        transaction.inject_failure(FailurePoint::PostValidation);
        let target = pbip.clone();
        // Something else restores the original bytes first; rollback has
        // nothing left to do and must not treat that as a failure.
        transaction.on_before_rollback(Box::new(move || {
            fs::write(&target, pbip_bytes(true)).expect("external revert");
        }));
        let error = transaction.commit().unwrap_err();

        assert_eq!(error.kind, SafeWriteErrorKind::RolledBack);
        assert_eq!(fs::read(&pbip).expect("read"), pbip_bytes(true));
        assert!(!temp.path().join(JOURNAL_DIR).exists());
    }

    #[test]
    fn external_change_during_backup_staging_is_caught_before_any_replacement() {
        let temp = TempDir::new("late-conflict");
        let pbip = project(temp.path(), true);
        let mut session = open(&pbip);
        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");

        // Inject after backups are durable, not before commit's initial scan.
        let target = temp.path().join("notes/unknown.txt");
        transaction.on_after_originals_staged(Box::new(move || write(&target, "changed late")));
        let mut before = tree(temp.path());
        before.get_mut("notes/unknown.txt").unwrap().0 = b"changed late".to_vec();
        let error = transaction.commit().unwrap_err();

        assert_eq!(error.kind, SafeWriteErrorKind::Conflict);
        assert_eq!(tree(temp.path()), before);
        assert!(auto_recovery_value(&pbip));
        assert!(!temp.path().join(JOURNAL_DIR).exists());
    }

    #[test]
    fn attributes_added_after_replacement_or_before_rollback_are_retained() {
        for during_rollback in [false, true] {
            let temp = TempDir::new("late-attribute");
            let pbip = project(temp.path(), true);
            let mut session = open(&pbip);
            let mut transaction = session.begin_transaction().unwrap();
            transaction.stage_auto_recovery(false).unwrap();
            let target = pbip.clone();
            let hook = Box::new(move || {
                let path = CString::new(target.as_os_str().as_bytes()).unwrap();
                // SAFETY: valid path/name strings and a one-byte value buffer.
                let result = unsafe {
                    libc::setxattr(
                        path.as_ptr(),
                        c"user.biforgeworks.external".as_ptr(),
                        b"x".as_ptr().cast(),
                        1,
                        0,
                    )
                };
                assert_eq!(
                    result,
                    0,
                    "set external xattr: {}",
                    std::io::Error::last_os_error()
                );
            });
            if during_rollback {
                transaction.inject_failure(FailurePoint::PostValidation);
                transaction.on_before_rollback(hook);
            } else {
                transaction.on_after_replace(hook);
            }
            let error = transaction.commit().unwrap_err();
            assert_eq!(error.kind, SafeWriteErrorKind::RecoveryRequired);
            assert!(
                !auto_recovery_value(&pbip),
                "rollback must not overwrite external attributes"
            );
            let path = CString::new(pbip.as_os_str().as_bytes()).unwrap();
            let mut value = [0u8; 1];
            // SAFETY: valid strings and writable one-byte output buffer.
            let size = unsafe {
                libc::getxattr(
                    path.as_ptr(),
                    c"user.biforgeworks.external".as_ptr(),
                    value.as_mut_ptr().cast(),
                    1,
                )
            };
            assert_eq!(size, 1);
            assert_eq!(value, [b'x']);
            assert!(temp
                .path()
                .join(JOURNAL_DIR)
                .join("originals/0000")
                .exists());
        }
    }

    #[test]
    fn an_externally_created_journal_blocks_opening_for_edit() {
        let temp = TempDir::new("external-journal");
        let pbip = project(temp.path(), true);
        fs::create_dir(temp.path().join(JOURNAL_DIR)).expect("mkdir");
        write(&temp.path().join(JOURNAL_DIR).join("something.txt"), "x");

        let mut session = open(&pbip);
        let recovery = session.pending_recovery().expect("reported");
        assert!(
            recovery.journal_unreadable,
            "unrecognized artifacts are not interpreted"
        );
        assert_eq!(session.state(), ProjectState::Error);
        let Err(blocked) = session.begin_transaction() else {
            panic!("unresolved recovery data must block transactions");
        };
        assert_eq!(blocked.kind, SafeWriteErrorKind::RecoveryPending);
        // Nothing unrecognized is deleted.
        assert!(temp.path().join(JOURNAL_DIR).join("something.txt").exists());
    }

    #[test]
    fn concurrent_saves_of_the_same_project_are_refused_across_aliases() {
        let temp = TempDir::new("concurrent");
        let pbip = project(temp.path(), true);
        let identity = fs::metadata(temp.path()).expect("stat");

        // Hold the lock as a concurrent save would, then try to commit
        // through a different path to the same directory.
        let _held = SaveLock::acquire(identity.dev(), identity.ino()).expect("lock acquired");
        let aliases = TempDir::new("concurrent-alias");
        let alias = aliases.path().join("project");
        symlink(temp.path(), &alias).expect("create root alias");

        let mut session = open(&alias.join("Sales.pbip"));
        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");
        let error = transaction.commit().unwrap_err();

        assert_eq!(error.kind, SafeWriteErrorKind::ConcurrentSave);
        assert_eq!(error.diagnostics[0].code, Code::ConcurrentSaveBlocked);
        assert!(auto_recovery_value(&pbip));
    }

    #[test]
    fn saving_twice_in_a_row_works_from_the_refreshed_snapshot() {
        let temp = TempDir::new("sequential");
        let pbip = project(temp.path(), true);
        let mut session = open(&pbip);

        for value in [false, true, false] {
            let mut transaction = session.begin_transaction().expect("transaction");
            transaction.stage_auto_recovery(value).expect("staged");
            let receipt = transaction.commit().expect("commit");
            assert_eq!(receipt.changed_files, vec!["Sales.pbip".to_owned()]);
            assert_eq!(auto_recovery_value(&pbip), value);
            assert_eq!(session.state(), ProjectState::Clean);
        }
        assert_eq!(fs::read(&pbip).expect("read"), pbip_bytes(false));
    }

    #[test]
    fn journal_records_targets_and_recoverable_originals() {
        let temp = TempDir::new("journal");
        let pbip = project(temp.path(), true);
        let mut session = open(&pbip);
        let mut transaction = session.begin_transaction().expect("transaction");
        transaction.stage_auto_recovery(false).expect("staged");
        transaction
            .stage_replacement_for_tests("notes/unknown.txt", b"rewritten".to_vec())
            .expect("staged");
        transaction.inject_failure(FailurePoint::RollbackRestore);
        let _ = transaction.commit().unwrap_err();

        let record = fs::read(temp.path().join(JOURNAL_DIR).join("journal.json")).expect("journal");
        let record = metadata::parse_object(&record).expect("journal is json");
        assert_eq!(record["format"].as_str(), Some("biforgeworks-save-journal"));
        assert_eq!(record["project_root"].as_str(), temp.path().to_str());
        let targets = record["targets"].as_array().expect("targets");
        assert_eq!(targets.len(), 2);
        assert!(targets
            .iter()
            .any(|target| target["path"] == "Sales.pbip" && target["replaced"] == true));
        let original = fs::read(
            temp.path()
                .join(JOURNAL_DIR)
                .join(targets[0]["original_file"].as_str().expect("path")),
        )
        .expect("original bytes retained");
        assert!(!original.is_empty());
    }
}
