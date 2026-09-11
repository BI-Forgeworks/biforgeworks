import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { AppShell } from './AppShell'
import type { PowerbiProjectSummary } from './api/powerbiProject'

afterEach(() => {
  cleanup()
})

const metadata = {
  name: 'BI Forgeworks',
  identifier: 'com.biforgeworks.desktop',
  version: '0.0.1',
  tagline: 'Linux-native analytics engineering',
  stage: 'Developer Preview',
}

const summary: PowerbiProjectSummary = {
  project_file: '/home/user/Reports/Sales.pbip',
  project_root: '/home/user/Reports',
  project_name: 'Sales',
  report: { path: 'Sales.Report', exists: true, format: 'PBIR' },
  semantic_model: { path: 'Sales.SemanticModel', exists: true, format: 'TMDL' },
  diagnostics: [],
}

const summaryWithDiagnostics: PowerbiProjectSummary = {
  ...summary,
  semantic_model: { path: null, exists: false, format: 'MISSING' },
  diagnostics: [
    {
      severity: 'error',
      code: 'SEMANTIC_MODEL_REFERENCE_MISSING',
      message: 'The semantic model reference could not be resolved.',
      path: null,
    },
    {
      severity: 'warning',
      code: 'UNKNOWN_REPORT_FORMAT',
      message: 'The report format could not be determined from its markers.',
      path: 'Sales.Report',
    },
  ],
}

function noop() {}

describe('AppShell', () => {
  it('renders a stable loading shell before metadata has loaded', () => {
    render(
      <AppShell
        metadata={null}
        project={null}
        isOpeningProject={false}
        projectError={null}
        onOpenProject={noop}
      />,
    )
    expect(screen.getByText(/Loading BI Forgeworks/i)).toBeTruthy()
  })

  it('renders the initial Power BI Engineering view with an Open Power BI Project action once metadata is provided', () => {
    render(
      <AppShell
        metadata={metadata}
        project={null}
        isOpeningProject={false}
        projectError={null}
        onOpenProject={noop}
      />,
    )

    expect(screen.getByText('BI Forgeworks')).toBeTruthy()
    expect(screen.getByText('Power BI Engineering')).toBeTruthy()
    expect(screen.getByText('Read-only')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Open Power BI Project' })).toBeTruthy()
    expect(screen.getByText(/No Power BI project opened yet/i)).toBeTruthy()
  })

  it('renders a successful project summary with paths, formats, and structural status', () => {
    render(
      <AppShell
        metadata={metadata}
        project={summary}
        isOpeningProject={false}
        projectError={null}
        onOpenProject={noop}
      />,
    )

    expect(screen.getByText('Sales')).toBeTruthy()
    expect(screen.getByText('/home/user/Reports/Sales.pbip')).toBeTruthy()
    expect(screen.getByText('/home/user/Reports')).toBeTruthy()
    expect(screen.getAllByText('Found')).toHaveLength(2)
    expect(screen.getByText('PBIR')).toBeTruthy()
    expect(screen.getByText('TMDL')).toBeTruthy()
    expect(screen.getByText(/No diagnostics reported/i)).toBeTruthy()
    // The read-only label persists once a project summary is shown, not just
    // on the initial empty screen.
    expect(screen.getByText('Read-only')).toBeTruthy()
  })

  it('renders diagnostics with severity, code, message, and path', () => {
    render(
      <AppShell
        metadata={metadata}
        project={summaryWithDiagnostics}
        isOpeningProject={false}
        projectError={null}
        onOpenProject={noop}
      />,
    )

    expect(screen.getByText('SEMANTIC_MODEL_REFERENCE_MISSING')).toBeTruthy()
    expect(
      screen.getByText('The semantic model reference could not be resolved.'),
    ).toBeTruthy()
    expect(screen.getByText('UNKNOWN_REPORT_FORMAT')).toBeTruthy()
    expect(screen.getAllByText('Sales.Report')).toHaveLength(2)
    expect(screen.getByText('Missing')).toBeTruthy()
    expect(screen.getByText('MISSING')).toBeTruthy()
  })

  it('disables the open button and marks it busy while opening', () => {
    render(
      <AppShell
        metadata={metadata}
        project={null}
        isOpeningProject
        projectError={null}
        onOpenProject={noop}
      />,
    )

    const button = screen.getByRole('button', { name: /Opening Power BI Project/i })
    expect(button.hasAttribute('disabled')).toBe(true)
    expect(button.getAttribute('aria-busy')).toBe('true')
  })

  it('renders an accessible alert when the command fails, without clearing a prior summary', () => {
    render(
      <AppShell
        metadata={metadata}
        project={summary}
        isOpeningProject={false}
        projectError="Failed to open the selected Power BI project."
        onOpenProject={noop}
      />,
    )

    expect(screen.getByRole('alert').textContent).toBe(
      'Failed to open the selected Power BI project.',
    )
    // The previously loaded summary remains visible after an error.
    expect(screen.getByText('Sales')).toBeTruthy()
  })

  it('invokes the open handler when the button is clicked', () => {
    const onOpenProject = vi.fn()

    render(
      <AppShell
        metadata={metadata}
        project={null}
        isOpeningProject={false}
        projectError={null}
        onOpenProject={onOpenProject}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Open Power BI Project' }))
    expect(onOpenProject).toHaveBeenCalledTimes(1)
  })
})
