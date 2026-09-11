import { describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { openPowerbiProject, selectPowerbiProject, type PowerbiProjectSummary } from './powerbiProject'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

describe('powerbiProject api', () => {
  it('invokes select_powerbi_project with no arguments and returns the chosen path', async () => {
    vi.mocked(invoke).mockResolvedValue('/home/user/Reports/Sales.pbip')

    const result = await selectPowerbiProject()

    expect(invoke).toHaveBeenCalledWith('select_powerbi_project')
    expect(result).toBe('/home/user/Reports/Sales.pbip')
  })

  it('resolves to null when the native picker is cancelled', async () => {
    vi.mocked(invoke).mockResolvedValue(null)

    const result = await selectPowerbiProject()

    expect(result).toBeNull()
  })

  it('invokes open_powerbi_project with the selected path and returns the typed summary', async () => {
    const summary: PowerbiProjectSummary = {
      project_file: '/home/user/Reports/Sales.pbip',
      project_root: '/home/user/Reports',
      project_name: 'Sales',
      report: { path: 'Sales.Report', exists: true, format: 'PBIR' },
      semantic_model: { path: 'Sales.SemanticModel', exists: true, format: 'TMDL' },
      diagnostics: [],
    }
    vi.mocked(invoke).mockResolvedValue(summary)

    const result = await openPowerbiProject('/home/user/Reports/Sales.pbip')

    expect(invoke).toHaveBeenCalledWith('open_powerbi_project', {
      path: '/home/user/Reports/Sales.pbip',
    })
    expect(result).toEqual(summary)
  })
})
