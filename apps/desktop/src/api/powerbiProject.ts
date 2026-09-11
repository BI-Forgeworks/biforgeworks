import { invoke } from '@tauri-apps/api/core'

export type PowerbiComponentFormat = 'PBIR' | 'PBIR_LEGACY' | 'TMDL' | 'TMSL' | 'UNKNOWN' | 'MISSING'

export type PowerbiDiagnosticSeverity = 'error' | 'warning' | 'info'

export interface PowerbiProjectComponent {
  path: string | null
  exists: boolean
  format: PowerbiComponentFormat
}

export interface PowerbiProjectDiagnostic {
  severity: PowerbiDiagnosticSeverity
  code: string
  message: string
  path: string | null
}

/**
 * Wire-format project summary returned by the `open_powerbi_project` Tauri
 * command. Field names are snake_case to match the Rust core crate's
 * serialized response; presentation components receive a mapped, camelCase
 * shape instead of this type directly.
 */
export interface PowerbiProjectSummary {
  project_file: string
  project_root: string
  project_name: string
  report: PowerbiProjectComponent
  semantic_model: PowerbiProjectComponent
  diagnostics: PowerbiProjectDiagnostic[]
}

/**
 * Opens the native file picker restricted to Power BI Project selection.
 * Resolves to `null` when the user cancels the picker without choosing a
 * file.
 */
export function selectPowerbiProject(): Promise<string | null> {
  return invoke<string | null>('select_powerbi_project')
}

/**
 * Discovers and summarizes the Power BI Project at `path`. Read-only: this
 * never writes to the selected project.
 */
export function openPowerbiProject(path: string): Promise<PowerbiProjectSummary> {
  return invoke<PowerbiProjectSummary>('open_powerbi_project', { path })
}
