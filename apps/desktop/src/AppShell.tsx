import { BrandHeader, ProjectExplorer, type ProjectExplorerData } from '@biforgeworks/ui'
import type { AppMetadata } from './api/appMetadata'
import type { PowerbiProjectSummary } from './api/powerbiProject'
import './App.css'

export interface AppShellProps {
  metadata: AppMetadata | null
  project: PowerbiProjectSummary | null
  isOpeningProject: boolean
  projectError: string | null
  onOpenProject: () => void
}

/**
 * Pure presentational shell: renders whatever metadata and project summary
 * it is given, or stable loading/empty states when none has arrived yet.
 * Holds no Tauri dependency, so it can be tested with plain injected props.
 */
export function AppShell({
  metadata,
  project,
  isOpeningProject,
  projectError,
  onOpenProject,
}: AppShellProps) {
  return (
    <main className="app-shell">
      {metadata ? (
        <>
          <BrandHeader name={metadata.name} tagline={metadata.tagline} stage={metadata.stage} />
          <section className="app-shell__workspace">
            <div className="app-shell__section-header">
              <h2 className="app-shell__section-title">Power BI Engineering</h2>
              <span
                className="app-shell__readonly-badge"
                aria-label="Read-only: opening a project never modifies it"
              >
                Read-only
              </span>
            </div>

            <button
              type="button"
              className="app-shell__open-button"
              onClick={onOpenProject}
              disabled={isOpeningProject}
              aria-busy={isOpeningProject}
            >
              {isOpeningProject ? 'Opening Power BI Project…' : 'Open Power BI Project'}
            </button>

            {projectError ? (
              <p role="alert" className="app-shell__error">
                {projectError}
              </p>
            ) : null}

            {project ? (
              <ProjectExplorer project={toProjectExplorerData(project)} />
            ) : (
              !projectError && (
                <p className="app-shell__empty">No Power BI project opened yet.</p>
              )
            )}
          </section>
        </>
      ) : (
        <p className="app-shell__loading">Loading BI Forgeworks…</p>
      )}
    </main>
  )
}

function toProjectExplorerData(summary: PowerbiProjectSummary): ProjectExplorerData {
  return {
    projectFile: summary.project_file,
    projectRoot: summary.project_root,
    projectName: summary.project_name,
    report: summary.report,
    semanticModel: summary.semantic_model,
    diagnostics: summary.diagnostics,
  }
}
