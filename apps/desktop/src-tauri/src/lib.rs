//! BI Forgeworks Tauri desktop shell.
//!
//! This crate is intentionally a thin presentation-layer shell: it exposes
//! Tauri commands that delegate to `biforgeworks-core` rather than holding
//! any application or domain logic itself.

use biforgeworks_core::app_metadata;
use serde::Serialize;

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
        .invoke_handler(tauri::generate_handler![get_app_metadata])
        .run(tauri::generate_context!())
        .expect("error while running BI Forgeworks desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

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
