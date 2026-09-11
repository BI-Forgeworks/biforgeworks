//! Minimal, framework-agnostic application metadata shared across BI Forgeworks
//! surfaces (Tauri backend, future CLI, future agent interfaces).

/// Canonical product display name.
pub const APP_NAME: &str = "BI Forgeworks";

/// Reverse-DNS application identifier used by the desktop shell.
pub const APP_IDENTIFIER: &str = "com.biforgeworks.desktop";

/// Current product version. Kept in sync with the Tauri and Cargo package
/// versions during WP00.
pub const APP_VERSION: &str = "0.0.1";

/// Product tagline shown in the desktop shell.
pub const APP_TAGLINE: &str = "Linux-native analytics engineering";

/// Current release stage shown in the desktop shell.
pub const APP_STAGE: &str = "Developer Preview";

/// Immutable snapshot of application identity metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppMetadata {
    pub name: &'static str,
    pub identifier: &'static str,
    pub version: &'static str,
    pub tagline: &'static str,
    pub stage: &'static str,
}

/// Returns the canonical application metadata.
///
/// This is the single source of truth for product identity; the Tauri shell
/// and any future presentation layers must read it rather than duplicating
/// these values.
pub fn app_metadata() -> AppMetadata {
    AppMetadata {
        name: APP_NAME,
        identifier: APP_IDENTIFIER,
        version: APP_VERSION,
        tagline: APP_TAGLINE,
        stage: APP_STAGE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_metadata_matches_constants() {
        let meta = app_metadata();
        assert_eq!(meta.name, APP_NAME);
        assert_eq!(meta.identifier, APP_IDENTIFIER);
        assert_eq!(meta.version, APP_VERSION);
        assert_eq!(meta.tagline, APP_TAGLINE);
        assert_eq!(meta.stage, APP_STAGE);
    }

    #[test]
    fn app_identifier_uses_biforgeworks_slug() {
        assert_eq!(APP_IDENTIFIER, "com.biforgeworks.desktop");
        assert!(!APP_IDENTIFIER.contains("biforge."));
    }

    #[test]
    fn app_metadata_is_copy_and_stable_across_calls() {
        let first = app_metadata();
        let second = app_metadata();
        assert_eq!(first, second);
    }
}
