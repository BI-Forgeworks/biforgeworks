use super::{
    diagnostics::{TmdlCode, TmdlDiagnostic},
    model::*,
    syntax::SourceSpan,
};
use crate::{
    fs_linux::{segment_name, Dir, EntryKind, FsError},
    ComponentFormat, Severity,
};
use std::path::{Component, Path};

const MAX_FILES: usize = 2048;
const MAX_ENTRIES: usize = 20_000;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

/// Discover the selected PBIP and inspect only its confined TMDL definition.
/// No source writes, timestamp-restoration writes, remote fetches, or execution.
pub fn inspect_project(path: &Path) -> SemanticInspection {
    let summary = crate::discover_project(path);
    if summary.semantic_model.format != ComponentFormat::Tmdl {
        return failure(
            InspectionStatus::Unsupported,
            TmdlCode::TmdlUnsupportedFormat,
            "Semantic inspection requires a local TMDL model; TMSL inspection is not yet supported",
            None,
        );
    }
    let load = || -> Result<Vec<(String, String)>, (String, FsError)> {
        let root_path = Path::new(&summary.project_root);
        let root = Dir::open_selected_root(root_path).map_err(|e| (String::new(), e))?;
        let model = Path::new(
            summary
                .semantic_model
                .path
                .as_deref()
                .ok_or_else(|| (String::new(), FsError::InvalidName))?,
        );
        let relative = model
            .strip_prefix(root_path)
            .map_err(|_| (String::new(), FsError::InvalidName))?;
        let mut dir = root.try_clone().map_err(|e| (String::new(), e))?;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err((String::new(), FsError::InvalidName));
            };
            use std::os::unix::ffi::OsStrExt;
            dir = dir
                .open_subdir(&segment_name(name.as_bytes()).map_err(|e| (String::new(), e))?)
                .map_err(|e| (relative.display().to_string(), e))?;
        }
        dir = dir
            .open_subdir(c"definition")
            .map_err(|e| ("definition".into(), e))?;
        let mut loader = Loader {
            sources: Vec::new(),
            bytes: 0,
            entries: 0,
        };
        loader.walk(&dir, "definition", 0)?;
        // Detect replacement of the selected root while the handles were in use.
        if root.stat_self().map_err(|e| (String::new(), e))?
            != Dir::open_selected_root(root_path)
                .and_then(|d| d.stat_self())
                .map_err(|e| (String::new(), e))?
        {
            return Err((String::new(), FsError::Unstable));
        }
        Ok(loader.sources)
    };
    match load() {
        Ok(sources) => super::inspect_sources(&summary.project_file, sources),
        Err((file, error)) => failure(
            InspectionStatus::Error,
            if error == FsError::TooLarge {
                TmdlCode::TmdlLimitExceeded
            } else {
                TmdlCode::TmdlReadFailed
            },
            &format!("TMDL source could not be read safely: {error:?}"),
            Some(SourceSpan {
                file,
                start: 0,
                end: 0,
                line: 1,
                column: 1,
            }),
        ),
    }
}

fn failure(
    status: InspectionStatus,
    code: TmdlCode,
    message: &str,
    source: Option<SourceSpan>,
) -> SemanticInspection {
    SemanticInspection {
        status,
        model: None,
        documents: vec![],
        diagnostics: vec![TmdlDiagnostic::new(Severity::Error, code, message, source)],
    }
}
struct Loader {
    sources: Vec<(String, String)>,
    bytes: usize,
    entries: usize,
}
impl Loader {
    fn walk(&mut self, dir: &Dir, prefix: &str, depth: usize) -> Result<(), (String, FsError)> {
        let wrap = |e| (prefix.to_owned(), e);
        if depth > 16 {
            return Err(wrap(FsError::TooLarge));
        }
        let before = dir.stat_self().map_err(wrap)?;
        let mut names = dir
            .entry_names(MAX_ENTRIES.saturating_sub(self.entries))
            .map_err(wrap)?;
        self.entries += names.len();
        names.sort();
        for bytes in names {
            let name = std::str::from_utf8(&bytes).map_err(|_| wrap(FsError::InvalidName))?;
            let relative = format!("{prefix}/{name}");
            let wrap = |e| (relative.clone(), e);
            let name_c = segment_name(&bytes).map_err(wrap)?;
            let stat = dir
                .stat_entry(&name_c)
                .map_err(wrap)?
                .ok_or_else(|| wrap(FsError::Unstable))?;
            match stat.kind {
                EntryKind::Directory => self.walk(
                    &dir.open_subdir(&name_c).map_err(wrap)?,
                    &relative,
                    depth + 1,
                )?,
                EntryKind::RegularFile if name.ends_with(".tmdl") => {
                    if self.sources.len() >= MAX_FILES {
                        return Err(wrap(FsError::TooLarge));
                    }
                    let limit = super::parser::MAX_SOURCE_BYTES
                        .min(MAX_TOTAL_BYTES.saturating_sub(self.bytes));
                    let bytes = dir.read_file(&name_c, limit as u64).map_err(wrap)?;
                    if dir.stat_entry(&name_c).map_err(wrap)? != Some(stat) {
                        return Err(wrap(FsError::Unstable));
                    }
                    self.bytes += bytes.len();
                    let text = String::from_utf8(bytes).map_err(|_| wrap(FsError::InvalidName))?;
                    self.sources.push((relative, text));
                }
                EntryKind::RegularFile => {}
                _ => return Err(wrap(FsError::Unsupported)),
            }
        }
        if dir.stat_self().map_err(wrap)? != before {
            return Err(wrap(FsError::Unstable));
        }
        Ok(())
    }
}
