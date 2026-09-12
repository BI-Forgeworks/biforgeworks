//! Discovery orchestration: `.pbip` → report folder → `definition.pbir` →
//! semantic-model folder, with format identification from structural
//! markers. All filesystem access goes through [`crate::fs_linux`].

use crate::fs_linux::{segment_name, Dir, EntryKind, FsError};
use crate::metadata::{self, DatasetReference, DefinitionVersion, JsonError, Object, SchemaStatus};
use crate::reference::{self, ReferenceError};
use crate::{
    ComponentFormat, Diagnostic, DiagnosticCode as Code, Entry, PowerBiProjectSummary,
    ProjectComponent, Severity, MAX_METADATA_BYTES,
};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Report,
    Model,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Report => "Report",
            Kind::Model => "Semantic model",
        }
    }

    fn definition_file(self) -> &'static str {
        match self {
            Kind::Report => "definition.pbir",
            Kind::Model => "definition.pbism",
        }
    }

    fn folder_not_found(self) -> Code {
        match self {
            Kind::Report => Code::ReportFolderNotFound,
            Kind::Model => Code::SemanticModelFolderNotFound,
        }
    }

    fn reference_invalid(self) -> Code {
        match self {
            Kind::Report => Code::ReportReferenceInvalid,
            Kind::Model => Code::SemanticModelReferenceInvalid,
        }
    }

    fn definition_missing(self) -> Code {
        match self {
            Kind::Report => Code::ReportDefinitionMissing,
            Kind::Model => Code::SemanticModelDefinitionMissing,
        }
    }

    fn definition_invalid_json(self) -> Code {
        match self {
            Kind::Report => Code::ReportDefinitionInvalidJson,
            Kind::Model => Code::SemanticModelDefinitionInvalidJson,
        }
    }

    fn definition_invalid_structure(self) -> Code {
        match self {
            Kind::Report => Code::ReportDefinitionInvalidStructure,
            Kind::Model => Code::SemanticModelDefinitionInvalidStructure,
        }
    }

    fn definition_schema_unsupported(self) -> Code {
        match self {
            Kind::Report => Code::ReportDefinitionSchemaUnsupported,
            Kind::Model => Code::SemanticModelDefinitionSchemaUnsupported,
        }
    }

    fn definition_version_unsupported(self) -> Code {
        match self {
            Kind::Report => Code::ReportDefinitionVersionUnsupported,
            Kind::Model => Code::SemanticModelDefinitionVersionUnsupported,
        }
    }

    fn schema_family(self) -> (&'static str, &'static [u64]) {
        match self {
            Kind::Report => (metadata::REPORT_DEFINITION_SCHEMA_FAMILY, &[1, 2]),
            Kind::Model => (metadata::MODEL_DEFINITION_SCHEMA_FAMILY, &[1]),
        }
    }

    fn component(self, summary: &mut PowerBiProjectSummary) -> &mut ProjectComponent {
        match self {
            Kind::Report => &mut summary.report,
            Kind::Model => &mut summary.semantic_model,
        }
    }
}

/// Outcome of probing a structural marker.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    Absent,
    Valid,
    /// Present but unusable (symlink, wrong type, unreadable); a diagnostic
    /// has been recorded.
    Invalid,
}

struct Discovery<'a> {
    root_path: PathBuf,
    summary: &'a mut PowerBiProjectSummary,
}

pub(crate) fn discover(entry: &Entry, summary: &mut PowerBiProjectSummary) {
    let mut discovery = Discovery {
        root_path: entry.root.clone(),
        summary,
    };
    discovery.run(entry);
}

impl Discovery<'_> {
    fn push(&mut self, severity: Severity, code: Code, message: impl Into<String>, path: String) {
        self.summary
            .diagnostics
            .push(Diagnostic::new(severity, code, message, Some(path)));
    }

    fn display(&self, segments: &[&str]) -> String {
        let mut path = self.root_path.clone();
        path.extend(segments);
        path.to_string_lossy().into_owned()
    }

    fn run(&mut self, entry: &Entry) {
        let pbip_path = self.summary.project_file.clone();

        let root = match Dir::open_selected_root(&entry.root) {
            Ok(root) => root,
            Err(err) => {
                let root_path = self.summary.project_root.clone();
                return match err {
                    FsError::NotFound | FsError::NotDirectory => self.push(
                        Severity::Error,
                        Code::PbipNotFound,
                        "The folder containing the .pbip file was not found.",
                        pbip_path,
                    ),
                    other => {
                        self.file_error(other, Code::PbipNotFound, "The project folder", root_path)
                    }
                };
            }
        };

        let bytes = match segment_name(entry.file_name.as_bytes())
            .and_then(|name| root.read_file(&name, MAX_METADATA_BYTES))
        {
            Ok(bytes) => bytes,
            Err(err) => {
                return self.file_error(err, Code::PbipNotFound, "The .pbip file", pbip_path)
            }
        };
        let Some(pbip) = self.parse(
            &bytes,
            Code::PbipInvalidJson,
            Code::PbipInvalidStructure,
            "The .pbip file",
            &pbip_path,
        ) else {
            return;
        };

        self.check_schema(
            &pbip,
            metadata::PBIP_SCHEMA_FAMILY,
            &[1],
            Code::PbipSchemaUnsupported,
            "The .pbip file",
            &pbip_path,
        );
        if !metadata::pbip_version_supported(&pbip) {
            self.push(
                Severity::Warning,
                Code::PbipVersionUnsupported,
                "The .pbip file's version is missing or not a supported version (1.x).",
                pbip_path.clone(),
            );
        }

        let (report_reference, unsupported_artifacts) = metadata::pbip_report_reference(&pbip);
        if unsupported_artifacts > 0 {
            self.push(
                Severity::Warning,
                Code::PbipArtifactUnsupported,
                format!(
                    "The .pbip file lists {unsupported_artifacts} artifact(s) that are not report references; they were ignored."
                ),
                pbip_path.clone(),
            );
        }

        let report_segments = match report_reference {
            metadata::ReportReference::Path(path) => {
                match self.resolve(
                    &[],
                    &path,
                    Kind::Report,
                    "The .pbip report reference",
                    &pbip_path,
                ) {
                    Some(segments) => segments,
                    None => return self.model_unreachable(),
                }
            }
            metadata::ReportReference::Missing => {
                self.push(
                    Severity::Error,
                    Code::ReportReferenceMissing,
                    "The .pbip file does not reference a report (artifacts[].report.path).",
                    pbip_path,
                );
                return self.model_unreachable();
            }
            metadata::ReportReference::Ambiguous => {
                self.push(
                    Severity::Error,
                    Code::ReportReferenceAmbiguous,
                    "The .pbip file references more than one report; the report to open cannot be chosen safely.",
                    pbip_path,
                );
                return self.model_unreachable();
            }
            metadata::ReportReference::Invalid => {
                self.push(
                    Severity::Error,
                    Code::ReportReferenceInvalid,
                    "The .pbip artifacts list or report reference does not have the documented shape.",
                    pbip_path,
                );
                return self.model_unreachable();
            }
        };

        let Some((dataset, pbir_path)) = self.discover_report(&root, &report_segments) else {
            return self.model_unreachable();
        };
        self.discover_model(&root, &report_segments, dataset, &pbir_path);
    }

    /// The model reference lives in `definition.pbir`, which could not be
    /// reached; record why the model is missing without repeating the cause.
    fn model_unreachable(&mut self) {
        let path = self.summary.project_file.clone();
        self.push(
            Severity::Warning,
            Code::SemanticModelReferenceMissing,
            "The semantic model reference could not be read because the report definition is unavailable.",
            path,
        );
    }

    fn resolve(
        &mut self,
        base: &[String],
        reference: &str,
        kind: Kind,
        what: &str,
        metadata_path: &str,
    ) -> Option<Vec<String>> {
        match reference::resolve(base, reference) {
            Ok(segments) => Some(segments),
            Err(err) => {
                let code = match err {
                    ReferenceError::OutsideProject => Code::ReferenceOutsideProject,
                    ReferenceError::Absolute => Code::ReferenceAbsolute,
                    _ => kind.reference_invalid(),
                };
                self.push(
                    Severity::Error,
                    code,
                    format!("{what} {}.", err.describe()),
                    metadata_path.to_owned(),
                );
                None
            }
        }
    }

    /// Opens a component folder, records it on the summary, and returns the
    /// handle when it is a real directory inside the project.
    fn open_component(&mut self, root: &Dir, segments: &[String], kind: Kind) -> Option<Dir> {
        let refs: Vec<&str> = segments.iter().map(String::as_str).collect();
        let folder = self.display(&refs);
        *kind.component(self.summary) = ProjectComponent {
            path: Some(folder.clone()),
            exists: false,
            format: ComponentFormat::Missing,
        };

        let walk = || -> Result<Dir, (FsError, usize)> {
            let mut current = root.try_clone().map_err(|e| (e, 0))?;
            for (index, segment) in segments.iter().enumerate() {
                let name = segment_name(segment.as_bytes()).map_err(|e| (e, index))?;
                current = current.open_subdir(&name).map_err(|e| (e, index))?;
            }
            Ok(current)
        };

        match walk() {
            Ok(dir) => {
                kind.component(self.summary).exists = true;
                Some(dir)
            }
            Err((err, index)) => {
                let at = self.display(&refs[..(index + 1).min(refs.len())]);
                let label = kind.label();
                match err {
                    FsError::NotFound => self.push(
                        Severity::Error,
                        kind.folder_not_found(),
                        format!("{label} folder was not found."),
                        folder,
                    ),
                    FsError::Symlink => self.push(
                        Severity::Error,
                        Code::SymlinkRejected,
                        format!("{label} folder path contains a symbolic link; symbolic links inside a project are not followed."),
                        at,
                    ),
                    FsError::NotDirectory => self.push(
                        Severity::Error,
                        Code::NotADirectory,
                        format!("{label} folder path contains an item that is not a directory."),
                        at,
                    ),
                    FsError::InvalidName => self.push(
                        Severity::Error,
                        kind.reference_invalid(),
                        format!("{label} folder path contains a name that cannot be opened."),
                        folder,
                    ),
                    other => self.file_error(other, kind.folder_not_found(), &format!("{label} folder"), at),
                }
                None
            }
        }
    }

    /// Reads and checks an item's outer definition file. Returns the parsed
    /// object and whether its `$schema`/`version` are trusted for format
    /// identification.
    fn read_definition(
        &mut self,
        dir: &Dir,
        segments: &[String],
        kind: Kind,
    ) -> Option<(Object, DefinitionVersion, String)> {
        let file = kind.definition_file();
        let mut refs: Vec<&str> = segments.iter().map(String::as_str).collect();
        refs.push(file);
        let path = self.display(&refs);
        let label = format!("{} {file}", kind.label());

        let bytes = match segment_name(file.as_bytes())
            .and_then(|name| dir.read_file(&name, MAX_METADATA_BYTES))
        {
            Ok(bytes) => bytes,
            Err(err) => {
                self.file_error(err, kind.definition_missing(), &label, path);
                return None;
            }
        };
        let object = self.parse(
            &bytes,
            kind.definition_invalid_json(),
            kind.definition_invalid_structure(),
            &label,
            &path,
        )?;

        let (family, majors) = kind.schema_family();
        let schema_ok = self.check_schema(
            &object,
            family,
            majors,
            kind.definition_schema_unsupported(),
            &label,
            &path,
        );
        let mut version = metadata::definition_version(&object);
        if version == DefinitionVersion::Unsupported {
            self.push(
                Severity::Warning,
                kind.definition_version_unsupported(),
                format!("{label} has a missing or unsupported version (supported: 1.0 and 4.x); the format was not identified."),
                path.clone(),
            );
        }
        if !schema_ok {
            version = DefinitionVersion::Unsupported;
        }
        Some((object, version, path))
    }

    fn discover_report(
        &mut self,
        root: &Dir,
        segments: &[String],
    ) -> Option<(DatasetReference, String)> {
        let dir = self.open_component(root, segments, Kind::Report)?;
        let Some((definition, version, pbir_path)) =
            self.read_definition(&dir, segments, Kind::Report)
        else {
            self.summary.report.format = ComponentFormat::Unknown;
            return None;
        };
        self.summary.report.format = match version {
            DefinitionVersion::Unsupported => ComponentFormat::Unknown,
            version => self.detect_report_format(&dir, segments, version),
        };
        Some((metadata::dataset_reference(&definition), pbir_path))
    }

    fn discover_model(
        &mut self,
        root: &Dir,
        report_segments: &[String],
        dataset: DatasetReference,
        pbir_path: &str,
    ) {
        let (code, message) = match dataset {
            DatasetReference::ByPath(path) => {
                let Some(segments) = self.resolve(
                    report_segments,
                    &path,
                    Kind::Model,
                    "The report's semantic model reference (datasetReference.byPath.path)",
                    pbir_path,
                ) else {
                    return;
                };
                let Some(dir) = self.open_component(root, &segments, Kind::Model) else {
                    return;
                };
                self.summary.semantic_model.format =
                    match self.read_definition(&dir, &segments, Kind::Model) {
                        None => ComponentFormat::Unknown,
                        Some((_, DefinitionVersion::Unsupported, _)) => ComponentFormat::Unknown,
                        Some((_, version, _)) => self.detect_model_format(&dir, &segments, version),
                    };
                return;
            }
            DatasetReference::ByConnection => {
                return self.push(
                    Severity::Warning,
                    Code::SemanticModelRemoteUnsupported,
                    "The report uses a remote semantic model (datasetReference.byConnection). Only local semantic models referenced by path are supported; connection details are not shown.",
                    pbir_path.to_owned(),
                );
            }
            DatasetReference::Missing => (
                Code::SemanticModelReferenceMissing,
                "The report definition does not reference a semantic model (datasetReference).",
            ),
            DatasetReference::Ambiguous => (
                Code::SemanticModelReferenceAmbiguous,
                "The report definition sets both byPath and byConnection; exactly one is allowed.",
            ),
            DatasetReference::Invalid => (
                Code::SemanticModelReferenceInvalid,
                "The report's datasetReference does not have the documented shape.",
            ),
        };
        self.push(Severity::Error, code, message, pbir_path.to_owned());
    }

    fn detect_report_format(
        &mut self,
        dir: &Dir,
        segments: &[String],
        version: DefinitionVersion,
    ) -> ComponentFormat {
        let folder = self.display(&segments.iter().map(String::as_str).collect::<Vec<_>>());
        let legacy = self.marker(dir, segments, &[], "report.json", EntryKind::RegularFile);
        let definition = self.marker(dir, segments, &[], "definition", EntryKind::Directory);

        if legacy != Marker::Absent && definition != Marker::Absent {
            self.push(
                Severity::Warning,
                Code::ReportFormatAmbiguous,
                "Report folder contains both a PBIR definition/ folder and a PBIR-Legacy report.json; the format is ambiguous.",
                folder,
            );
            return ComponentFormat::Unknown;
        }
        if definition != Marker::Absent {
            if version == DefinitionVersion::LegacyOnly {
                self.push(
                    Severity::Warning,
                    Code::ReportFormatVersionMismatch,
                    "Report has a PBIR definition/ folder but definition.pbir version 1.0 permits only PBIR-Legacy.",
                    folder,
                );
                return ComponentFormat::Unknown;
            }
            if definition == Marker::Invalid {
                return ComponentFormat::Unknown;
            }
            return self.detect_pbir(dir, segments, &folder);
        }
        match legacy {
            Marker::Valid => ComponentFormat::PbirLegacy,
            Marker::Invalid => ComponentFormat::Unknown,
            Marker::Absent => {
                self.push(
                    Severity::Warning,
                    Code::UnknownReportFormat,
                    "Report folder has neither a PBIR definition/ folder nor a PBIR-Legacy report.json.",
                    folder,
                );
                ComponentFormat::Unknown
            }
        }
    }

    fn detect_pbir(&mut self, dir: &Dir, segments: &[String], folder: &str) -> ComponentFormat {
        let Some(definition) = self.open_marker_dir(dir, segments, "definition") else {
            return ComponentFormat::Unknown;
        };
        let report = self.marker(
            &definition,
            segments,
            &["definition"],
            "report.json",
            EntryKind::RegularFile,
        );
        let version = self.marker(
            &definition,
            segments,
            &["definition"],
            "version.json",
            EntryKind::RegularFile,
        );
        if report == Marker::Invalid || version == Marker::Invalid {
            return ComponentFormat::Unknown;
        }
        if report == Marker::Absent || version == Marker::Absent {
            self.push(
                Severity::Warning,
                Code::UnknownReportFormat,
                "Report definition/ folder lacks definition/report.json or definition/version.json required by PBIR.",
                folder.to_owned(),
            );
            return ComponentFormat::Unknown;
        }

        let mut refs: Vec<&str> = segments.iter().map(String::as_str).collect();
        refs.extend(["definition", "version.json"]);
        let path = self.display(&refs);
        let label = "PBIR definition/version.json";
        let bytes = match definition.read_file(c"version.json", MAX_METADATA_BYTES) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.file_error(err, Code::ReportVersionMetadataInvalid, label, path);
                return ComponentFormat::Unknown;
            }
        };
        let Some(object) = self.parse(
            &bytes,
            Code::ReportVersionMetadataInvalid,
            Code::ReportVersionMetadataInvalid,
            label,
            &path,
        ) else {
            return ComponentFormat::Unknown;
        };
        let schema_ok = self.check_schema(
            &object,
            metadata::VERSION_METADATA_SCHEMA_FAMILY,
            &[1],
            Code::ReportVersionMetadataUnsupported,
            label,
            &path,
        );
        if !metadata::pbir_version_supported(&object) {
            self.push(
                Severity::Warning,
                Code::ReportVersionMetadataUnsupported,
                "PBIR definition/version.json has a missing or unsupported version (supported: 1.x.0 and 2.x.0); the format was not identified.",
                path,
            );
            return ComponentFormat::Unknown;
        }
        if schema_ok {
            ComponentFormat::Pbir
        } else {
            ComponentFormat::Unknown
        }
    }

    fn detect_model_format(
        &mut self,
        dir: &Dir,
        segments: &[String],
        version: DefinitionVersion,
    ) -> ComponentFormat {
        let folder = self.display(&segments.iter().map(String::as_str).collect::<Vec<_>>());
        let bim = self.marker(dir, segments, &[], "model.bim", EntryKind::RegularFile);
        let definition = self.marker(dir, segments, &[], "definition", EntryKind::Directory);

        if bim != Marker::Absent && definition != Marker::Absent {
            self.push(
                Severity::Warning,
                Code::ModelFormatAmbiguous,
                "Semantic model folder contains both a TMDL definition/ folder and a TMSL model.bim; the format is ambiguous.",
                folder,
            );
            return ComponentFormat::Unknown;
        }
        if definition != Marker::Absent {
            if version == DefinitionVersion::LegacyOnly {
                self.push(
                    Severity::Warning,
                    Code::ModelFormatVersionMismatch,
                    "Semantic model has a TMDL definition/ folder but definition.pbism version 1.0 permits only TMSL.",
                    folder,
                );
                return ComponentFormat::Unknown;
            }
            if definition == Marker::Invalid {
                return ComponentFormat::Unknown;
            }
            let Some(tmdl) = self.open_marker_dir(dir, segments, "definition") else {
                return ComponentFormat::Unknown;
            };
            return match self.marker(
                &tmdl,
                segments,
                &["definition"],
                "model.tmdl",
                EntryKind::RegularFile,
            ) {
                Marker::Valid => ComponentFormat::Tmdl,
                Marker::Invalid => ComponentFormat::Unknown,
                Marker::Absent => {
                    self.push(
                        Severity::Warning,
                        Code::UnknownModelFormat,
                        "Semantic model definition/ folder lacks definition/model.tmdl required by TMDL.",
                        folder,
                    );
                    ComponentFormat::Unknown
                }
            };
        }
        match bim {
            Marker::Valid => ComponentFormat::Tmsl,
            Marker::Invalid => ComponentFormat::Unknown,
            Marker::Absent => {
                self.push(
                    Severity::Warning,
                    Code::UnknownModelFormat,
                    "Semantic model folder has neither a TMDL definition/ folder nor a TMSL model.bim.",
                    folder,
                );
                ComponentFormat::Unknown
            }
        }
    }

    /// Probes a marker without opening or reading it.
    fn marker(
        &mut self,
        dir: &Dir,
        segments: &[String],
        below: &[&str],
        name: &str,
        expected: EntryKind,
    ) -> Marker {
        let mut refs: Vec<&str> = segments.iter().map(String::as_str).collect();
        refs.extend(below);
        refs.push(name);
        let path = self.display(&refs);
        let kind = segment_name(name.as_bytes()).and_then(|c_name| dir.entry_kind(&c_name));
        match kind {
            Ok(EntryKind::Missing) => Marker::Absent,
            Ok(kind) if kind == expected => Marker::Valid,
            Ok(EntryKind::Symlink) => {
                self.push(
                    Severity::Warning,
                    Code::SymlinkRejected,
                    "Format marker is a symbolic link; symbolic links inside a project are not followed.",
                    path,
                );
                Marker::Invalid
            }
            Ok(_) => {
                let (code, message) = if expected == EntryKind::Directory {
                    (Code::NotADirectory, "Format marker should be a directory.")
                } else {
                    (
                        Code::NotARegularFile,
                        "Format marker should be a regular file.",
                    )
                };
                self.push(Severity::Warning, code, message, path);
                Marker::Invalid
            }
            Err(err) => {
                self.file_error(err, Code::IoError, "Format marker", path);
                Marker::Invalid
            }
        }
    }

    fn open_marker_dir(&mut self, dir: &Dir, segments: &[String], name: &str) -> Option<Dir> {
        let result = segment_name(name.as_bytes()).and_then(|c_name| dir.open_subdir(&c_name));
        match result {
            Ok(dir) => Some(dir),
            Err(err) => {
                let mut refs: Vec<&str> = segments.iter().map(String::as_str).collect();
                refs.push(name);
                let path = self.display(&refs);
                self.file_error(err, Code::IoError, "Format marker folder", path);
                None
            }
        }
    }

    /// Parses metadata, recording a diagnostic on failure.
    fn parse(
        &mut self,
        bytes: &[u8],
        invalid_json: Code,
        invalid_structure: Code,
        label: &str,
        path: &str,
    ) -> Option<Object> {
        match metadata::parse_object(bytes) {
            Ok(object) => Some(object),
            Err(err) => {
                let code = match err {
                    JsonError::InvalidUtf8 => Code::MetadataInvalidUtf8,
                    JsonError::InvalidJson { .. } => invalid_json,
                    JsonError::NotAnObject => invalid_structure,
                };
                self.push(
                    Severity::Error,
                    code,
                    format!("{label} {}.", err.describe()),
                    path.to_owned(),
                );
                None
            }
        }
    }

    /// Records schema diagnostics; returns false when the declared schema is
    /// unsupported (contents must not be treated as validated).
    fn check_schema(
        &mut self,
        object: &Object,
        family: &str,
        majors: &[u64],
        unsupported: Code,
        label: &str,
        path: &str,
    ) -> bool {
        match metadata::schema_status(object, family, majors) {
            SchemaStatus::Supported => true,
            SchemaStatus::Absent => {
                self.push(
                    Severity::Warning,
                    Code::MetadataSchemaMissing,
                    format!("{label} does not declare a $schema."),
                    path.to_owned(),
                );
                true
            }
            SchemaStatus::Unsupported => {
                self.push(
                    Severity::Warning,
                    unsupported,
                    format!("{label} declares an unrecognized or unsupported $schema; it was not treated as valid."),
                    path.to_owned(),
                );
                false
            }
        }
    }

    fn file_error(&mut self, err: FsError, missing: Code, label: &str, path: String) {
        let (code, message) = match err {
            FsError::NotFound => (missing, format!("{label} was not found.")),
            FsError::Symlink => (
                Code::SymlinkRejected,
                format!("{label} is a symbolic link; symbolic links inside a project are not followed."),
            ),
            FsError::NotRegularFile | FsError::NotDirectory => (
                Code::NotARegularFile,
                format!("{label} is not a regular file."),
            ),
            FsError::TooLarge => (
                Code::MetadataTooLarge,
                format!("{label} is larger than the {MAX_METADATA_BYTES}-byte metadata limit and was not read."),
            ),
            FsError::AccessDenied => (Code::FileAccessDenied, format!("{label} could not be accessed (permission denied).")),
            FsError::NoAtimeUnavailable => (
                Code::ReadOnlyGuaranteeUnavailable,
                format!("{label} could not be opened without updating its access time (Linux O_NOATIME requires owning the file), so it was not read."),
            ),
            FsError::InvalidName
            | FsError::Io
            | FsError::AlreadyExists
            | FsError::Unstable
            | FsError::Unsupported => (Code::IoError, format!("{label} could not be read.")),
        };
        self.push(Severity::Error, code, message, path);
    }
}
