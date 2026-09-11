//! Lexical resolution of untrusted, project-relative references.
//!
//! PBIP and PBIR references are documented as relative, `/`-separated paths.
//! Resolution happens purely on strings against a base expressed as folder
//! names below the project root, so a reference can never be resolved to a
//! location outside the root. The filesystem walk that follows opens each
//! resulting segment without following symlinks, which keeps the lexical
//! result and the physical location in agreement.

/// Longest reference string accepted, in bytes.
const MAX_REFERENCE_BYTES: usize = 1024;
/// Longest single path segment accepted, in bytes (typical `NAME_MAX`).
const MAX_SEGMENT_BYTES: usize = 255;
/// Deepest resolved folder accepted, in segments below the project root.
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReferenceError {
    Empty,
    TooLong,
    TooDeep,
    InvalidCharacter,
    /// Unix-absolute, Windows drive-qualified, UNC, or URI-style reference.
    Absolute,
    /// `\` separators; the documented separator is `/`.
    Backslash,
    /// `..` climbs above the project root.
    OutsideProject,
}

impl ReferenceError {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::Empty => "is empty",
            Self::TooLong => "is unreasonably long",
            Self::TooDeep => "is nested unreasonably deep",
            Self::InvalidCharacter => "contains control characters",
            Self::Absolute => {
                "is not relative; absolute, drive-qualified, UNC, and URI references are not supported"
            }
            Self::Backslash => "uses '\\' separators; Power BI references must use '/'",
            Self::OutsideProject => "points outside the project folder",
        }
    }
}

/// Resolves `reference` relative to `base` (segments below the project root)
/// and returns the target's segments below the project root.
pub(crate) fn resolve(base: &[String], reference: &str) -> Result<Vec<String>, ReferenceError> {
    if reference.is_empty() {
        return Err(ReferenceError::Empty);
    }
    if reference.len() > MAX_REFERENCE_BYTES {
        return Err(ReferenceError::TooLong);
    }
    if reference.chars().any(char::is_control) {
        return Err(ReferenceError::InvalidCharacter);
    }
    // `/x`, `//server/share`, `\x`, `\\server\share`, `\\?\C:\x`, `C:\x`,
    // `C:x`, and `file:///x` are all rejected here. A ':' is never valid in
    // a relative Windows path, so any colon is treated as qualification.
    if reference.starts_with('/') || reference.starts_with('\\') || reference.contains(':') {
        return Err(ReferenceError::Absolute);
    }
    if reference.contains('\\') {
        return Err(ReferenceError::Backslash);
    }

    let mut resolved: Vec<String> = base.to_vec();
    for segment in reference.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if resolved.pop().is_none() {
                    return Err(ReferenceError::OutsideProject);
                }
            }
            name => {
                if name.len() > MAX_SEGMENT_BYTES {
                    return Err(ReferenceError::TooLong);
                }
                resolved.push(name.to_owned());
                if resolved.len() > MAX_DEPTH {
                    return Err(ReferenceError::TooDeep);
                }
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn resolves_plain_relative_reference() {
        assert_eq!(resolve(&[], "Sales.Report"), Ok(base(&["Sales.Report"])));
        assert_eq!(
            resolve(&[], "./nested/Sales.Report/"),
            Ok(base(&["nested", "Sales.Report"]))
        );
    }

    #[test]
    fn resolves_sibling_through_parent() {
        assert_eq!(
            resolve(&base(&["Sales.Report"]), "../Sales.SemanticModel"),
            Ok(base(&["Sales.SemanticModel"]))
        );
        assert_eq!(
            resolve(&base(&["a", "Sales.Report"]), "../../b/Model"),
            Ok(base(&["b", "Model"]))
        );
    }

    #[test]
    fn rejects_escape_above_root() {
        assert_eq!(
            resolve(&base(&["Sales.Report"]), "../../Other.SemanticModel"),
            Err(ReferenceError::OutsideProject)
        );
        assert_eq!(resolve(&[], ".."), Err(ReferenceError::OutsideProject));
        // Leaving and re-entering the root is still an escape.
        assert_eq!(
            resolve(&base(&["R"]), "../../root/R"),
            Err(ReferenceError::OutsideProject)
        );
    }

    #[test]
    fn rejects_absolute_forms() {
        for reference in [
            "/etc/passwd",
            "//server/share/Model",
            "\\\\server\\share\\Model",
            "\\\\?\\C:\\Model",
            "\\Model",
            "C:\\Models\\Sales",
            "C:/Models/Sales",
            "c:Sales",
            "file:///etc/passwd",
            "Sales.Report:stream",
        ] {
            assert_eq!(
                resolve(&[], reference),
                Err(ReferenceError::Absolute),
                "{reference}"
            );
        }
    }

    #[test]
    fn rejects_backslash_empty_control_and_oversized() {
        assert_eq!(
            resolve(&[], "..\\Sales.SemanticModel"),
            Err(ReferenceError::Backslash)
        );
        assert_eq!(resolve(&[], ""), Err(ReferenceError::Empty));
        assert_eq!(resolve(&[], "a\0b"), Err(ReferenceError::InvalidCharacter));
        assert_eq!(resolve(&[], "a\nb"), Err(ReferenceError::InvalidCharacter));
        assert_eq!(
            resolve(&[], &"a".repeat(MAX_REFERENCE_BYTES + 1)),
            Err(ReferenceError::TooLong)
        );
        assert_eq!(
            resolve(&[], &"a".repeat(MAX_SEGMENT_BYTES + 1)),
            Err(ReferenceError::TooLong)
        );
        assert_eq!(
            resolve(&[], &vec!["a"; MAX_DEPTH + 1].join("/")),
            Err(ReferenceError::TooDeep)
        );
    }

    #[test]
    fn reference_to_root_itself_is_inside() {
        assert_eq!(resolve(&base(&["R"]), ".."), Ok(Vec::new()));
    }
}
