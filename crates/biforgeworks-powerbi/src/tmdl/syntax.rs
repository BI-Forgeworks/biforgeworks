//! Source-preserving syntax. All offsets are UTF-8 byte offsets, end-exclusive.
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceSpan {
    pub file: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Expression {
    /// Exact source bytes (including indentation and newline style), without fences.
    pub raw: String,
    /// Presentation only: removes the structural indentation, never used to write.
    pub display: String,
    pub span: SourceSpan,
    pub fenced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxKind {
    Object,
    Property,
    Reference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyntaxNode {
    pub kind: SyntaxKind,
    pub keyword: String,
    pub name: Option<String>,
    pub value: Option<String>,
    pub expression: Option<Expression>,
    pub description: Option<String>,
    pub span: SourceSpan,
    pub header: SourceSpan,
    pub declarations: Vec<SourceSpan>,
    pub children: Vec<SyntaxNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceDocument {
    pub file: String,
    /// Includes BOM, comments, whitespace, and unsupported syntax verbatim.
    pub text: String,
    pub nodes: Vec<SyntaxNode>,
    /// Invalid documents retain source/syntax but are excluded from semantic projection.
    pub is_valid: bool,
}
