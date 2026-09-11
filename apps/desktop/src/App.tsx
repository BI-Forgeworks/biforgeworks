import { useEffect, useState } from 'react'
import { getAppMetadata, type AppMetadata } from './api/appMetadata'
import { AppShell } from './AppShell'

export function App() {
  const [metadata, setMetadata] = useState<AppMetadata | null>(null)

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

  return <AppShell metadata={metadata} />
}
