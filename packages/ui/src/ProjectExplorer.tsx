export type ProjectComponentFormat = 'PBIR' | 'PBIR_LEGACY' | 'TMDL' | 'TMSL' | 'UNKNOWN' | 'MISSING'

export type ProjectDiagnosticSeverity = 'error' | 'warning' | 'info'

export interface ProjectComponentSummary {
  path: string | null
  exists: boolean
  format: ProjectComponentFormat
}

export interface ProjectDiagnosticSummary {
  severity: ProjectDiagnosticSeverity
  code: string
  message: string
  path: string | null
}

export interface ProjectExplorerData {
  projectFile: string
  projectRoot: string
  projectName: string
  report: ProjectComponentSummary
  semanticModel: ProjectComponentSummary
  diagnostics: ProjectDiagnosticSummary[]
}

export interface ProjectExplorerProps {
  project: ProjectExplorerData
}

/**
 * Presentation-only, read-only summary of a discovered Power BI Project.
 * Shows only project/report/semantic-model paths, formats, structural
 * status, and diagnostics — no lower-level objects and no editing controls.
 */
export function ProjectExplorer({ project }: ProjectExplorerProps) {
  return (
    <section className="project-explorer" aria-label="Power BI project summary">
      <header className="project-explorer__header">
        <h2 className="project-explorer__name">{project.projectName}</h2>
        <dl className="project-explorer__meta">
          <dt>Project file</dt>
          <dd>{project.projectFile}</dd>
          <dt>Project root</dt>
          <dd>{project.projectRoot}</dd>
        </dl>
      </header>

      <div className="project-explorer__components">
        <ProjectComponentCard title="Report" component={project.report} />
        <ProjectComponentCard title="Semantic Model" component={project.semanticModel} />
      </div>

      <DiagnosticsList diagnostics={project.diagnostics} />
    </section>
  )
}

function ProjectComponentCard({
  title,
  component,
}: {
  title: string
  component: ProjectComponentSummary
}) {
  return (
    <article className="project-component-card">
      <h3 className="project-component-card__title">{title}</h3>
      <dl className="project-component-card__meta">
        <dt>Path</dt>
        <dd>{component.path ?? 'Not available'}</dd>
        <dt>Status</dt>
        <dd>{component.exists ? 'Found' : 'Missing'}</dd>
        <dt>Format</dt>
        <dd>{component.format}</dd>
      </dl>
    </article>
  )
}

function DiagnosticsList({ diagnostics }: { diagnostics: ProjectDiagnosticSummary[] }) {
  return (
    <div className="project-diagnostics">
      <h3 className="project-diagnostics__title">Diagnostics</h3>
      {diagnostics.length === 0 ? (
        <p className="project-diagnostics__empty">No diagnostics reported.</p>
      ) : (
        <ul className="project-diagnostics__list">
          {diagnostics.map((diagnostic, index) => (
            <li
              key={`${diagnostic.code}-${index}`}
              className={`project-diagnostic project-diagnostic--${diagnostic.severity}`}
            >
              <span className="project-diagnostic__severity">{diagnostic.severity}</span>
              <span className="project-diagnostic__code">{diagnostic.code}</span>
              <span className="project-diagnostic__message">{diagnostic.message}</span>
              {diagnostic.path ? <span className="project-diagnostic__path">{diagnostic.path}</span> : null}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
