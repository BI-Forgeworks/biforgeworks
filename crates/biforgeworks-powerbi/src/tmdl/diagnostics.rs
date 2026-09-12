use super::syntax::SourceSpan;
use crate::Severity;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TmdlCode {
    TmdlSyntaxError,
    TmdlUnknownProperty,
    TmdlDuplicateObject,
    TmdlUnresolvedReference,
    TmdlInvalidRelationshipReference,
    TmdlInvalidHierarchyReference,
    TmdlUnsupportedObject,
    TmdlReadFailed,
    TmdlLimitExceeded,
    TmdlUnsupportedFormat,
}

#[derive(Debug, Clone, Serialize)]
pub struct TmdlDiagnostic {
    pub severity: Severity,
    pub code: TmdlCode,
    pub message: String,
    pub source: Option<SourceSpan>,
}

impl TmdlDiagnostic {
    pub(crate) fn new(
        severity: Severity,
        code: TmdlCode,
        message: impl Into<String>,
        source: Option<SourceSpan>,
    ) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            source,
        }
    }
}
