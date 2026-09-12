import { useCallback, useEffect, useRef, useState } from 'react'
import { getAppMetadata, type AppMetadata } from './api/appMetadata'
import {
  openPowerbiProject,
  selectPowerbiProject,
  type PowerbiProjectSummary,
} from './api/powerbiProject'
import { AppShell } from './AppShell'
import { inspectPowerbiSemanticModel, type SemanticInspection } from './api/semanticModel'

// Tauri commands here return `Result<T, String>`, so a command failure
// rejects with a plain string rather than an `Error` instance.
function describeProjectError(error: unknown): string {
  if (typeof error === 'string') {
    return error
  }
  if (error instanceof Error) {
    return error.message
  }
  return 'Failed to open the selected Power BI project.'
}

export function App() {
  const [metadata, setMetadata] = useState<AppMetadata | null>(null)
  const [project, setProject] = useState<PowerbiProjectSummary | null>(null)
  const [inspection, setInspection] = useState<SemanticInspection | null>(null)
  const [inspectionError, setInspectionError] = useState<string | null>(null)
  const [isOpeningProject, setIsOpeningProject] = useState(false)
  const [projectError, setProjectError] = useState<string | null>(null)
  const isOpeningRef = useRef(false)

  useEffect(() => {
    let cancelled = false

    getAppMetadata()
      .then((result) => {
        if (!cancelled) {
          setMetadata(result)
        }
      })
      .catch((error: unknown) => {
        console.error('Failed to load BI Forgeworks application metadata', error)
      })

    return () => {
      cancelled = true
    }
  }, [])

  const handleOpenProject = useCallback(() => {
    // Guard with a ref (in addition to the disabled button) so a rapid
    // double-click can never start a second concurrent open.
    if (isOpeningRef.current) {
      return
    }
    isOpeningRef.current = true
    setIsOpeningProject(true)
    setProjectError(null)

    void (async () => {
      try {
        const path = await selectPowerbiProject()
        if (path === null) {
          // Selection cancelled: preserve whatever project summary was
          // already displayed.
          return
        }

        const summary = await openPowerbiProject(path)
        setProject(summary)
        setInspection(null)
        setInspectionError(null)
        try {
          setInspection(await inspectPowerbiSemanticModel())
        } catch (error: unknown) {
          setInspectionError(describeProjectError(error))
        }
      } catch (error: unknown) {
        console.error('Failed to open Power BI project', error)
        setProjectError(describeProjectError(error))
      } finally {
        isOpeningRef.current = false
        setIsOpeningProject(false)
      }
    })()
  }, [])

  return (
    <AppShell
      metadata={metadata}
      project={project}
      isOpeningProject={isOpeningProject}
      projectError={projectError}
      onOpenProject={handleOpenProject}
      inspection={inspection}
      inspectionError={inspectionError}
    />
  )
}
