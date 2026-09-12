import { invoke } from '@tauri-apps/api/core'

export interface SourceSpan { file: string; start: number; end: number; line: number; column: number }
export interface Expression { raw: string; display: string; span: SourceSpan; fenced: boolean }
export interface Property { name: string; value: string; source: SourceSpan }
export interface ObjectMetadata {
  id: string; name: string; description: string | null; lineage_tag: string | null
  is_hidden: boolean; properties: Property[]; annotations: Property[]; sources: SourceSpan[]
}
export interface Column { metadata: ObjectMetadata; kind: 'data' | 'calculated' | 'calculated_table'; data_type: string | null; source_column: string | null; expression: Expression | null; format_string: string | null; sort_by_column: string | null }
export interface Measure { metadata: ObjectMetadata; expression: Expression | null; format_string: string | null; display_folder: string | null }
export interface HierarchyLevel { metadata: ObjectMetadata; ordinal: number; column: string | null }
export interface Hierarchy { metadata: ObjectMetadata; levels: HierarchyLevel[] }
export interface Partition { metadata: ObjectMetadata; mode: string | null; source_kind: string | null; expression: Expression | null }
export interface Table { metadata: ObjectMetadata; columns: Column[]; measures: Measure[]; hierarchies: Hierarchy[]; partitions: Partition[] }
export interface ObjectReference { table: string; object: string; resolved: boolean }
export interface Relationship { metadata: ObjectMetadata; from: ObjectReference | null; to: ObjectReference | null; from_cardinality: string; to_cardinality: string; cross_filter: string; is_active: boolean; security_filter: string | null }
export interface ColumnPermission { metadata: ObjectMetadata; permission: string | null }
export interface RoleMember { metadata: ObjectMetadata; member_type: string | null; identity_provider: string | null }
export interface TableFilter { metadata: ObjectMetadata; table: string; expression: Expression | null; resolved: boolean; column_permissions: ColumnPermission[] }
export interface Role { metadata: ObjectMetadata; model_permission: string | null; filters: TableFilter[]; members: RoleMember[] }
export interface PerspectiveTable { metadata: ObjectMetadata; table: string; resolved: boolean; columns: string[]; measures: string[]; hierarchies: string[] }
export interface Perspective { metadata: ObjectMetadata; tables: PerspectiveTable[] }
export interface Translation { target: string; properties: Property[]; source: SourceSpan }
export interface Culture { metadata: ObjectMetadata; translations: Translation[]; linguistic_metadata: Expression | null }
export interface NamedExpression { metadata: ObjectMetadata; kind: string | null; expression: Expression | null }
export interface ModelFunction { metadata: ObjectMetadata; expression: Expression | null }
export interface SemanticModel {
  metadata: ObjectMetadata; database: ObjectMetadata | null; tables: Table[]; relationships: Relationship[]
  roles: Role[]; perspectives: Perspective[]; cultures: Culture[]; expressions: NamedExpression[]; functions: ModelFunction[]
}
export interface TmdlDiagnostic { severity: 'error' | 'warning' | 'info'; code: string; message: string; source: SourceSpan | null }
export interface SemanticInspection { status: 'ready' | 'partial' | 'unsupported' | 'error'; model: SemanticModel | null; diagnostics: TmdlDiagnostic[] }

/** The backend owns the selected project; this command accepts no filesystem path. */
export function inspectPowerbiSemanticModel(): Promise<SemanticInspection> {
  return invoke<SemanticInspection>('inspect_powerbi_semantic_model')
}
