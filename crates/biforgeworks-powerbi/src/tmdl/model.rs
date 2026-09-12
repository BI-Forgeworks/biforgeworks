//! Power BI-specific inspection types; opaque syntax is retained separately.
use super::{
    diagnostics::TmdlDiagnostic,
    syntax::{Expression, SourceDocument, SourceSpan},
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub source: SourceSpan,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectMetadata {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub lineage_tag: Option<String>,
    pub is_hidden: bool,
    pub properties: Vec<Property>,
    pub annotations: Vec<Property>,
    /// Multiple declarations may contribute to the same logical object.
    pub sources: Vec<SourceSpan>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SemanticModel {
    pub metadata: ObjectMetadata,
    pub database: Option<ObjectMetadata>,
    pub tables: Vec<Table>,
    pub relationships: Vec<Relationship>,
    pub roles: Vec<Role>,
    pub perspectives: Vec<Perspective>,
    pub cultures: Vec<Culture>,
    pub expressions: Vec<NamedExpression>,
    pub functions: Vec<ModelFunction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Table {
    pub metadata: ObjectMetadata,
    pub columns: Vec<Column>,
    pub measures: Vec<Measure>,
    pub hierarchies: Vec<Hierarchy>,
    pub partitions: Vec<Partition>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Data,
    Calculated,
    CalculatedTable,
}
#[derive(Debug, Clone, Serialize)]
pub struct Column {
    pub metadata: ObjectMetadata,
    pub kind: ColumnKind,
    pub data_type: Option<String>,
    pub source_column: Option<String>,
    pub expression: Option<Expression>,
    pub format_string: Option<String>,
    pub sort_by_column: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Measure {
    pub metadata: ObjectMetadata,
    pub expression: Option<Expression>,
    pub format_string: Option<String>,
    pub display_folder: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Hierarchy {
    pub metadata: ObjectMetadata,
    pub levels: Vec<HierarchyLevel>,
}
#[derive(Debug, Clone, Serialize)]
pub struct HierarchyLevel {
    pub metadata: ObjectMetadata,
    pub ordinal: usize,
    pub column: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Partition {
    pub metadata: ObjectMetadata,
    pub mode: Option<String>,
    pub source_kind: Option<String>,
    pub expression: Option<Expression>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ObjectReference {
    pub table: String,
    pub object: String,
    pub resolved: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Relationship {
    pub metadata: ObjectMetadata,
    pub from: Option<ObjectReference>,
    pub to: Option<ObjectReference>,
    pub from_cardinality: String,
    pub to_cardinality: String,
    pub cross_filter: String,
    pub is_active: bool,
    pub security_filter: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Role {
    pub metadata: ObjectMetadata,
    pub model_permission: Option<String>,
    pub filters: Vec<TableFilter>,
    pub members: Vec<RoleMember>,
}
#[derive(Debug, Clone, Serialize)]
pub struct RoleMember {
    pub metadata: ObjectMetadata,
    pub member_type: Option<String>,
    pub identity_provider: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ColumnPermission {
    pub metadata: ObjectMetadata,
    pub permission: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct TableFilter {
    pub metadata: ObjectMetadata,
    pub table: String,
    pub expression: Option<Expression>,
    pub resolved: bool,
    pub column_permissions: Vec<ColumnPermission>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Perspective {
    pub metadata: ObjectMetadata,
    pub tables: Vec<PerspectiveTable>,
}
#[derive(Debug, Clone, Serialize)]
pub struct PerspectiveTable {
    pub metadata: ObjectMetadata,
    pub table: String,
    pub resolved: bool,
    pub columns: Vec<String>,
    pub measures: Vec<String>,
    pub hierarchies: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Culture {
    pub metadata: ObjectMetadata,
    pub translations: Vec<Translation>,
    pub linguistic_metadata: Option<Expression>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Translation {
    pub target: String,
    pub properties: Vec<Property>,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, Serialize)]
pub struct NamedExpression {
    pub metadata: ObjectMetadata,
    pub kind: Option<String>,
    pub expression: Option<Expression>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ModelFunction {
    pub metadata: ObjectMetadata,
    pub expression: Option<Expression>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionStatus {
    Ready,
    Partial,
    Unsupported,
    Error,
}
#[derive(Debug, Clone, Serialize)]
pub struct SemanticInspection {
    pub status: InspectionStatus,
    pub model: Option<SemanticModel>,
    pub diagnostics: Vec<TmdlDiagnostic>,
    /// Complete source retained in Rust; not copied to the UI DTO.
    #[serde(skip)]
    pub documents: Vec<SourceDocument>,
}
