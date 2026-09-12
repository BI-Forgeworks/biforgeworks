//! Power BI Project (`.pbip`) discovery and safe, explicit writes.
//!
//! Discovery ([`discover_project`]) is strictly read-only and always has
//! been: it updates no content, metadata, or access times. Writing is a
//! separate, opt-in path in [`safe_writes`], where a session snapshots the
//! whole project, stages an edit to one managed file, validates it, refuses
//! to proceed if anything changed underneath, and replaces the file
//! atomically with the original retained for rollback.
//!
//! This crate locates a project's report and semantic-model folders, follows
//! the documented outer metadata (`.pbip` → `definition.pbir` →
//! `datasetReference.byPath`), and identifies each component's storage format
//! from documented structural markers. The separate [`tmdl`] reader projects
//! TMDL semantic objects while retaining source and opaque DAX/M. TMSL and PBIR
//! pages/visuals are not parsed. Discovery never writes to,
//! or updates timestamps of, anything inside the selected project.
//!
//! All project metadata is treated as untrusted. Discovery problems are reported
//! as [`Diagnostic`] values on [`PowerBiProjectSummary`]; the safe-write API
//! returns typed errors with reusable diagnostics.
//!
//! There is no general write API. The only content this crate will change is
//! `settings.enableAutoRecovery` in a `.pbip`, spliced over that boolean's
//! byte span so everything else in the file survives exactly.

use serde::Serialize;
use std::path::Path;

pub mod tmdl;

// Only the Linux discovery path consumes these today.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod metadata;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod reference;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod json_span;

#[cfg(target_os = "linux")]
mod discovery;
#[cfg(target_os = "linux")]
mod fs_linux;
#[cfg(target_os = "linux")]
pub mod safe_writes;
#[cfg(target_os = "linux")]
pub mod snapshot;

/// Largest metadata file (`.pbip`, `definition.pbir`, `definition.pbism`,
/// `definition/version.json`) that discovery will read. Real files are a few
/// hundred bytes; anything larger is reported rather than read.
pub const MAX_METADATA_BYTES: u64 = 1024 * 1024;

/// Typed, serializable result of opening a `.pbip` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PowerBiProjectSummary {
    /// The `.pbip` file as selected (made absolute, not canonicalized).
    pub project_file: String,
    /// Directory containing the `.pbip`; every reference must stay inside it.
    pub project_root: String,
    /// The `.pbip` file stem.
    pub project_name: String,
    pub report: ProjectComponent,
    pub semantic_model: ProjectComponent,
    pub diagnostics: Vec<Diagnostic>,
}

/// A discovered report or semantic-model folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectComponent {
    /// Resolved folder path, when a reference was resolved inside the project.
    pub path: Option<String>,
    /// Whether a real (non-symlink) directory exists at `path`.
    pub exists: bool,
    pub format: ComponentFormat,
}

impl ProjectComponent {
    fn missing() -> Self {
        Self {
            path: None,
            exists: false,
            format: ComponentFormat::Missing,
        }
    }
}

/// Storage format identified from structural markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComponentFormat {
    /// Enhanced report format: `definition/` with `report.json` and
    /// `version.json`.
    Pbir,
    /// Legacy report format: `report.json` beside `definition.pbir`.
    PbirLegacy,
    /// Semantic model as a TMDL `definition/` folder with `model.tmdl`.
    Tmdl,
    /// Semantic model as a TMSL `model.bim` file.
    Tmsl,
    /// A folder exists but its format could not be identified with
    /// confidence (missing, conflicting, or unsupported markers/versions).
    Unknown,
    /// No usable local folder was found.
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// Stable diagnostic codes. Serialized as `SCREAMING_SNAKE_CASE` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiagnosticCode {
    // Entry `.pbip`.
    PbipInvalidPath,
    PbipInvalidExtension,
    PbipNotFound,
    PbipInvalidJson,
    PbipInvalidStructure,
    PbipSchemaUnsupported,
    PbipVersionUnsupported,
    PbipArtifactUnsupported,
    // Report.
    ReportReferenceMissing,
    ReportReferenceInvalid,
    ReportReferenceAmbiguous,
    ReportFolderNotFound,
    ReportDefinitionMissing,
    ReportDefinitionInvalidJson,
    ReportDefinitionInvalidStructure,
    ReportDefinitionSchemaUnsupported,
    ReportDefinitionVersionUnsupported,
    ReportVersionMetadataInvalid,
    ReportVersionMetadataUnsupported,
    UnknownReportFormat,
    ReportFormatAmbiguous,
    ReportFormatVersionMismatch,
    // Semantic model.
    SemanticModelReferenceMissing,
    SemanticModelReferenceInvalid,
    SemanticModelReferenceAmbiguous,
    SemanticModelRemoteUnsupported,
    SemanticModelFolderNotFound,
    SemanticModelDefinitionMissing,
    SemanticModelDefinitionInvalidJson,
    SemanticModelDefinitionInvalidStructure,
    SemanticModelDefinitionSchemaUnsupported,
    SemanticModelDefinitionVersionUnsupported,
    UnknownModelFormat,
    ModelFormatAmbiguous,
    ModelFormatVersionMismatch,
    // References and filesystem safety.
    ReferenceOutsideProject,
    ReferenceAbsolute,
    SymlinkRejected,
    NotADirectory,
    NotARegularFile,
    MetadataTooLarge,
    MetadataInvalidUtf8,
    MetadataSchemaMissing,
    PathNotUtf8,
    FileAccessDenied,
    IoError,
    ReadOnlyGuaranteeUnavailable,
    // Safe writes (WP02).
    /// The project tree could not be recorded completely.
    SnapshotIncomplete,
    /// The project exceeds the recordable entry, depth, or byte limits.
    SnapshotTooLarge,
    /// `settings.enableAutoRecovery` is absent or not a boolean.
    AutoRecoverySettingUnavailable,
    /// Staged bytes failed their own validation.
    StagedContentInvalid,
    /// A destination is not a file this crate may replace.
    UnsafeWriteTarget,
    /// The project as discovered is not in a state this crate will save.
    ProjectNotSaveable,
    ExternalFileChanged,
    ExternalFileAdded,
    ExternalFileRemoved,
    ExternalFileReplaced,
    ExternalFileTypeChanged,
    /// Another save for the same project is running in this process.
    ConcurrentSaveBlocked,
    /// A write or rename failed.
    WriteFailed,
    /// Rediscovery after a save no longer matched the project it opened.
    PostSaveValidationFailed,
    /// A failed save was undone.
    SaveRolledBack,
    /// A replaced file could not be restored.
    RollbackFailed,
    /// Recovery data from an interrupted or failed save must be resolved.
    RecoveryRequired,
    /// Recovery data could not be cleaned up; nothing unknown was deleted.
    RecoveryArtifactsRetained,
}

/// A user-facing finding. Messages never echo untrusted metadata values
/// (reference strings, connection strings, or parser excerpts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub path: Option<String>,
}

impl Diagnostic {
    pub(crate) fn new(
        severity: Severity,
        code: DiagnosticCode,
        message: impl Into<String>,
        path: Option<String>,
    ) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            path,
        }
    }
}

/// Discovers the Power BI Project whose `.pbip` file is at `path`.
///
/// Never panics on untrusted input and never writes to the project: every
/// problem is reported in [`PowerBiProjectSummary::diagnostics`]. On Linux,
/// metadata is read through descriptor-relative, no-follow, `O_NOATIME`
/// opens; if a file cannot be read without updating its access time,
/// discovery fails closed with `READ_ONLY_GUARANTEE_UNAVAILABLE`. Other
/// platforms currently always fail closed with that diagnostic.
pub fn discover_project(path: &Path) -> PowerBiProjectSummary {
    let entry = Entry::from_path(path);
    let mut summary = PowerBiProjectSummary {
        project_file: entry.project_file.clone(),
        project_root: entry.project_root.clone(),
        project_name: entry.project_name.clone(),
        report: ProjectComponent::missing(),
        semantic_model: ProjectComponent::missing(),
        diagnostics: Vec::new(),
    };

    if entry.not_utf8 {
        summary.diagnostics.push(Diagnostic::new(
            Severity::Info,
            DiagnosticCode::PathNotUtf8,
            "The project path is not valid UTF-8; displayed paths replace invalid bytes.",
            Some(summary.project_file.clone()),
        ));
    }
    if let Some(problem) = entry.problem {
        summary.diagnostics.push(problem);
        return summary;
    }

    platform_discover(&entry, &mut summary);
    summary
}

#[cfg(target_os = "linux")]
fn platform_discover(entry: &Entry, summary: &mut PowerBiProjectSummary) {
    discovery::discover(entry, summary);
}

#[cfg(not(target_os = "linux"))]
fn platform_discover(_entry: &Entry, summary: &mut PowerBiProjectSummary) {
    summary.diagnostics.push(Diagnostic::new(
        Severity::Error,
        DiagnosticCode::ReadOnlyGuaranteeUnavailable,
        "Read-only project discovery is currently supported only on Linux.",
        Some(summary.project_file.clone()),
    ));
}

/// Lexically validated entry path. Nothing here touches the filesystem.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct Entry {
    pub(crate) root: std::path::PathBuf,
    pub(crate) file_name: std::ffi::OsString,
    pub(crate) project_file: String,
    pub(crate) project_root: String,
    pub(crate) project_name: String,
    pub(crate) not_utf8: bool,
    pub(crate) problem: Option<Diagnostic>,
}

impl Entry {
    fn from_path(path: &Path) -> Self {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let root = absolute.parent().map(Path::to_path_buf).unwrap_or_default();
        let file_name = path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        let project_file = absolute.to_string_lossy().into_owned();
        let not_utf8 = absolute.to_str().is_none();

        let problem = if path.as_os_str().is_empty() || path.file_name().is_none() {
            Some(Diagnostic::new(
                Severity::Error,
                DiagnosticCode::PbipInvalidPath,
                "The selected path does not name a .pbip file.",
                (!path.as_os_str().is_empty()).then(|| project_file.clone()),
            ))
        } else if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pbip"))
        {
            Some(Diagnostic::new(
                Severity::Error,
                DiagnosticCode::PbipInvalidExtension,
                "The selected file is not a Power BI Project (.pbip) file.",
                Some(project_file.clone()),
            ))
        } else {
            None
        };

        Self {
            project_root: root.to_string_lossy().into_owned(),
            project_name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            root,
            file_name,
            project_file,
            not_utf8,
            problem,
        }
    }
}
