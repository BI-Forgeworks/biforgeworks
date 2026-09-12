//! BI Forgeworks Tauri desktop shell.
//!
//! This crate is intentionally a thin presentation-layer shell: it exposes
//! Tauri commands that delegate to reusable Rust crates rather than holding
//! any application or domain logic itself.

use biforgeworks_core::app_metadata;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri_plugin_dialog::DialogExt;

#[derive(Default, Clone)]
struct OpenProject(Arc<Mutex<Option<String>>>);

impl OpenProject {
    fn open(&self, path: String) -> Result<biforgeworks_powerbi::PowerBiProjectSummary, String> {
        let summary = biforgeworks_powerbi::discover_project(std::path::Path::new(&path));
        *self.0.lock().map_err(|_| "Project state unavailable")? =
            Some(summary.project_file.clone());
        Ok(summary)
    }
    fn inspect(&self) -> Result<biforgeworks_powerbi::tmdl::SemanticInspection, String> {
        let path = self
            .0
            .lock()
            .map_err(|_| "Project state unavailable")?
            .clone()
            .ok_or("Open a Power BI project first")?;
        Ok(biforgeworks_powerbi::tmdl::inspect_project(
            std::path::Path::new(&path),
        ))
    }
}

/// Inspects the current backend-owned selection; accepts no filesystem path.
#[tauri::command]
async fn inspect_powerbi_semantic_model(
    state: tauri::State<'_, OpenProject>,
) -> Result<biforgeworks_powerbi::tmdl::SemanticInspection, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.inspect())
        .await
        .map_err(|_| "Semantic inspection could not finish".to_string())?
}

/// A filtered native picker; the frontend gets no general dialog or filesystem API.
#[tauri::command]
async fn select_powerbi_project(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Open Power BI Project")
            .add_filter("Power BI Project", &["pbip"])
            .blocking_pick_file()
            .map(|selected| {
                let path = selected
                    .into_path()
                    .map_err(|_| "The selected item is not a local filesystem path.".to_string())?;
                path.into_os_string()
                    .into_string()
                    .map_err(|_| "The selected path is not valid UTF-8.".to_string())
            })
            .transpose()
    })
    .await
    .map_err(|_| "The project picker could not finish.".to_string())?
}

/// All input handling and discovery belong to the reusable Power BI crate.
#[tauri::command]
async fn open_powerbi_project(
    path: String,
    state: tauri::State<'_, OpenProject>,
) -> Result<biforgeworks_powerbi::PowerBiProjectSummary, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.open(path))
        .await
        .map_err(|_| "Project discovery could not finish.".to_string())?
}

/// Wire-format snapshot of `biforgeworks_core::AppMetadata` returned to the
/// frontend over the Tauri command bridge.
#[derive(Debug, Clone, Serialize)]
pub struct AppMetadataResponse {
    pub name: String,
    pub identifier: String,
    pub version: String,
    pub tagline: String,
    pub stage: String,
}

/// Returns application identity metadata sourced from `biforgeworks-core`.
///
/// This command exists to prove the Tauri backend depends on and calls the
/// core crate rather than duplicating product identity locally.
#[tauri::command]
fn get_app_metadata() -> AppMetadataResponse {
    let meta = app_metadata();
    AppMetadataResponse {
        name: meta.name.to_string(),
        identifier: meta.identifier.to_string(),
        version: meta.version.to_string(),
        tagline: meta.tagline.to_string(),
        stage: meta.stage.to_string(),
    }
}

/// Builds and runs the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .manage(OpenProject::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_app_metadata,
            select_powerbi_project,
            open_powerbi_project,
            inspect_powerbi_semantic_model
        ])
        .run(tauri::generate_context!())
        .expect("error while running BI Forgeworks desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn open_project_command_delegates_to_powerbi_crate() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/powerbi/valid-pbir-tmdl/Sales.pbip");
        let expected = biforgeworks_powerbi::discover_project(&path);
        let actual = OpenProject::default()
            .open(path.to_str().expect("fixture path is UTF-8").to_owned())
            .expect("discovery command completes");
        assert_eq!(actual, expected);
        assert!(actual.report.exists);
        assert!(actual.semantic_model.exists);
        assert!(actual.diagnostics.is_empty());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn semantic_inspection_uses_only_opened_project() {
        let state = OpenProject::default();
        assert!(state.inspect().is_err());
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/powerbi/tmdl-star-schema/Sales.pbip");
        state.open(path.to_str().unwrap().into()).unwrap();
        let actual = state.inspect().unwrap();
        let expected = biforgeworks_powerbi::tmdl::inspect_project(&path);
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    }

    #[test]
    fn get_app_metadata_command_calls_core_crate() {
        let response = get_app_metadata();
        let core_meta = biforgeworks_core::app_metadata();

        assert_eq!(response.name, core_meta.name);
        assert_eq!(response.identifier, core_meta.identifier);
        assert_eq!(response.version, core_meta.version);
        assert_eq!(response.tagline, core_meta.tagline);
        assert_eq!(response.stage, core_meta.stage);
    }

    #[test]
    fn get_app_metadata_command_reports_biforgeworks_identity() {
        let response = get_app_metadata();

        assert_eq!(response.name, "BI Forgeworks");
        assert_eq!(response.identifier, "com.biforgeworks.desktop");
        assert_eq!(response.version, "0.0.1");
    }
}
