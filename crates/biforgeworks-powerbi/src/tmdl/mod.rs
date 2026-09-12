//! Read-only, source-aware TMDL inspection. DAX and M are opaque text.
pub mod diagnostics;
mod lexer;
pub mod model;
pub mod parser;
mod projection;
#[cfg(target_os = "linux")]
mod reader;
pub mod syntax;

pub use model::*;
#[cfg(target_os = "linux")]
pub use reader::inspect_project;

/// Parse already-loaded UTF-8 source without any filesystem access.
pub fn inspect_sources(
    project_identity: &str,
    sources: Vec<(String, String)>,
) -> SemanticInspection {
    use diagnostics::{TmdlCode, TmdlDiagnostic};
    const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
    if sources.len() > 2048
        || sources
            .iter()
            .try_fold(0usize, |n, (_, text)| n.checked_add(text.len()))
            .is_none_or(|n| n > MAX_TOTAL_BYTES)
    {
        return SemanticInspection {
            status: InspectionStatus::Error,
            model: None,
            documents: vec![],
            diagnostics: vec![TmdlDiagnostic::new(
                crate::Severity::Error,
                TmdlCode::TmdlLimitExceeded,
                "TMDL model exceeds the source count or total byte budget",
                None,
            )],
        };
    }
    let mut documents = Vec::new();
    let mut diagnostics = Vec::new();
    fn nodes(items: &[syntax::SyntaxNode]) -> usize {
        items.iter().map(|n| 1 + nodes(&n.children)).sum()
    }
    let mut total_nodes = 0;
    for (file, text) in sources {
        let (document, mut errors) = parser::parse_document(&file, text);
        total_nodes += nodes(&document.nodes);
        documents.push(document);
        diagnostics.append(&mut errors);
        if total_nodes > parser::MAX_NODES {
            diagnostics.push(TmdlDiagnostic::new(
                crate::Severity::Error,
                TmdlCode::TmdlLimitExceeded,
                "TMDL model exceeds the total syntax node budget",
                None,
            ));
            return SemanticInspection {
                status: InspectionStatus::Error,
                model: None,
                documents,
                diagnostics,
            };
        }
    }
    let model = projection::project(project_identity, &documents, &mut diagnostics);
    let status = if diagnostics
        .iter()
        .any(|d| d.severity == crate::Severity::Error)
    {
        InspectionStatus::Partial
    } else {
        InspectionStatus::Ready
    };
    SemanticInspection {
        status,
        model: Some(model),
        documents,
        diagnostics,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn inspect_project(_path: &std::path::Path) -> SemanticInspection {
    SemanticInspection {
        status: InspectionStatus::Unsupported,
        model: None,
        documents: vec![],
        diagnostics: vec![diagnostics::TmdlDiagnostic::new(
            crate::Severity::Error,
            diagnostics::TmdlCode::TmdlReadFailed,
            "Timestamp-preserving semantic inspection currently requires Linux",
            None,
        )],
    }
}
