import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { App } from './App'
import type { PowerbiProjectSummary } from './api/powerbiProject'

vi.mock('./api/appMetadata', () => ({
  getAppMetadata: vi.fn().mockResolvedValue({
    name: 'BI Forgeworks',
    identifier: 'com.biforgeworks.desktop',
    version: '0.0.1',
    tagline: 'Linux-native analytics engineering',
    stage: 'Developer Preview',
  }),
}))

const selectPowerbiProject = vi.fn()
const openPowerbiProject = vi.fn()
vi.mock('./api/semanticModel', () => ({
  inspectPowerbiSemanticModel: vi.fn().mockResolvedValue({ status: 'ready', model: null, diagnostics: [] }),
}))

vi.mock('./api/powerbiProject', () => ({
  selectPowerbiProject: (...args: unknown[]) => selectPowerbiProject(...args),
  openPowerbiProject: (...args: unknown[]) => openPowerbiProject(...args),
}))

const summary: PowerbiProjectSummary = {
  project_file: '/home/user/Reports/Sales.pbip',
  project_root: '/home/user/Reports',
  project_name: 'Sales',
  report: { path: 'Sales.Report', exists: true, format: 'PBIR' },
  semantic_model: { path: 'Sales.SemanticModel', exists: true, format: 'TMDL' },
  diagnostics: [],
}

const secondSummary: PowerbiProjectSummary = {
  ...summary,
  project_name: 'Marketing',
  project_file: '/home/user/Reports/Marketing.pbip',
  project_root: '/home/user/Reports',
}

afterEach(() => {
  cleanup()
  selectPowerbiProject.mockReset()
  openPowerbiProject.mockReset()
})

describe('App', () => {
  it('loads application metadata through the command boundary and renders the shell', async () => {
    render(<App />)

    expect(await screen.findByText('BI Forgeworks')).toBeTruthy()
    expect(screen.getByText('Linux-native analytics engineering')).toBeTruthy()
    expect(screen.getByText('Developer Preview')).toBeTruthy()
    expect(screen.getByText(/No Power BI project opened yet/i)).toBeTruthy()
  })

  it('opens a project and renders its summary on success', async () => {
    selectPowerbiProject.mockResolvedValue('/home/user/Reports/Sales.pbip')
    openPowerbiProject.mockResolvedValue(summary)

    render(<App />)
    await screen.findByText('BI Forgeworks')

    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))

    expect(await screen.findByText('Sales')).toBeTruthy()
    expect(openPowerbiProject).toHaveBeenCalledWith('/home/user/Reports/Sales.pbip')
  })

  it('renders diagnostics returned from a successful open', async () => {
    selectPowerbiProject.mockResolvedValue('/home/user/Reports/Sales.pbip')
    openPowerbiProject.mockResolvedValue({
      ...summary,
      diagnostics: [
        {
          severity: 'warning',
          code: 'UNKNOWN_MODEL_FORMAT',
          message: 'The semantic model format could not be determined.',
          path: 'Sales.SemanticModel',
        },
      ],
    })

    render(<App />)
    await screen.findByText('BI Forgeworks')

    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))

    expect(await screen.findByText('UNKNOWN_MODEL_FORMAT')).toBeTruthy()
    expect(
      screen.getByText('The semantic model format could not be determined.'),
    ).toBeTruthy()
  })

  it('preserves the prior project summary when selection is cancelled', async () => {
    selectPowerbiProject.mockResolvedValueOnce('/home/user/Reports/Sales.pbip')
    openPowerbiProject.mockResolvedValueOnce(summary)

    render(<App />)
    await screen.findByText('BI Forgeworks')

    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))
    expect(await screen.findByText('Sales')).toBeTruthy()

    selectPowerbiProject.mockResolvedValueOnce(null)
    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Open Power BI Project' })).toBeTruthy()
    })
    expect(openPowerbiProject).toHaveBeenCalledTimes(1)
    expect(screen.getByText('Sales')).toBeTruthy()
  })

  it('keeps the empty state without discovery when the first selection is cancelled', async () => {
    selectPowerbiProject.mockResolvedValue(null)
    render(<App />)
    fireEvent.click(await screen.findByRole('button', { name: 'Open Power BI Project' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Open Power BI Project' }).hasAttribute('disabled')).toBe(false)
    })
    expect(openPowerbiProject).not.toHaveBeenCalled()
    expect(screen.getByText(/No Power BI project opened yet/i)).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows an accessible error and preserves the prior summary when the command fails', async () => {
    selectPowerbiProject.mockResolvedValueOnce('/home/user/Reports/Sales.pbip')
    openPowerbiProject.mockResolvedValueOnce(summary)

    render(<App />)
    await screen.findByText('BI Forgeworks')

    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))
    expect(await screen.findByText('Sales')).toBeTruthy()

    selectPowerbiProject.mockResolvedValueOnce('/home/user/Reports/Broken.pbip')
    openPowerbiProject.mockRejectedValueOnce(new Error('Failed to read the selected project.'))
    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))

    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'Failed to read the selected project.',
    )
    expect(screen.getByText('Sales')).toBeTruthy()
  })

  it('surfaces a plain string command rejection, matching the Result<T, String> Tauri contract', async () => {
    selectPowerbiProject.mockResolvedValueOnce('/home/user/Reports/Sales.pbip')
    openPowerbiProject.mockResolvedValueOnce(summary)

    render(<App />)
    await screen.findByText('BI Forgeworks')

    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))
    expect(await screen.findByText('Sales')).toBeTruthy()

    selectPowerbiProject.mockResolvedValueOnce('/home/user/Reports/Broken.pbip')
    openPowerbiProject.mockRejectedValueOnce('Project discovery could not finish.')
    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))

    expect(await screen.findByRole('alert')).toHaveProperty(
      'textContent',
      'Project discovery could not finish.',
    )
    expect(screen.getByText('Sales')).toBeTruthy()
  })

  it('prevents duplicate concurrent opens from a rapid double click', async () => {
    let resolveSelect!: (value: string | null) => void
    selectPowerbiProject.mockReturnValue(
      new Promise((resolve) => {
        resolveSelect = resolve
      }),
    )

    render(<App />)
    await screen.findByText('BI Forgeworks')

    const button = screen.getByRole('button', { name: 'Open Power BI Project' })
    fireEvent.click(button)
    fireEvent.click(button)
    fireEvent.click(button)

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Opening Power BI Project/i })).toBeTruthy()
    })
    expect(selectPowerbiProject).toHaveBeenCalledTimes(1)

    resolveSelect(null)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Open Power BI Project' })).toBeTruthy()
    })
  })

  it('replaces the summary when a second project is opened successfully', async () => {
    selectPowerbiProject.mockResolvedValueOnce('/home/user/Reports/Sales.pbip')
    openPowerbiProject.mockResolvedValueOnce(summary)

    render(<App />)
    await screen.findByText('BI Forgeworks')

    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))
    expect(await screen.findByText('Sales')).toBeTruthy()

    selectPowerbiProject.mockResolvedValueOnce('/home/user/Reports/Marketing.pbip')
    openPowerbiProject.mockResolvedValueOnce(secondSummary)
    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))

    expect(await screen.findByText('Marketing')).toBeTruthy()
    expect(screen.queryByText('Sales')).toBeNull()
  })
})
