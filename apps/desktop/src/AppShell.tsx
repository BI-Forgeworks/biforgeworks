import { BrandHeader } from '@biforgeworks/ui'
import type { AppMetadata } from './api/appMetadata'
import './App.css'

export interface AppShellProps {
  metadata: AppMetadata | null
}

/**
 * Pure presentational shell: renders whatever metadata it is given, or a
 * stable loading state when none has arrived yet. Holds no Tauri dependency,
 * so it can be tested with plain injected props.
 */
export function AppShell({ metadata }: AppShellProps) {
  return (
    <main className="app-shell">
      {metadata ? (
        <BrandHeader name={metadata.name} tagline={metadata.tagline} stage={metadata.stage} />
      ) : (
        <p className="app-shell__loading">Loading BI Forgeworks…</p>
      )}
    </main>
  )
}
