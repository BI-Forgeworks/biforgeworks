import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { SemanticExplorer } from './SemanticExplorer'
import type { ObjectMetadata, SemanticInspection } from './api/semanticModel'

const span = { file: 'tables/Sales.tmdl', start: 0, end: 20, line: 3, column: 1 }
const metadata = (name: string): ObjectMetadata => ({ id: `test:${name}`, name, description: null, lineage_tag: null, is_hidden: false, properties: [], annotations: [], sources: [span] })
const expression = { raw: '\t\t\tSUM(Sales[Quantity])\n', display: 'SUM(Sales[Quantity])\n', span, fenced: false }
const inspection: SemanticInspection = {
  status: 'ready', diagnostics: [], model: {
    metadata: metadata('Model'), database: metadata('Database'),
    tables: [{ metadata: metadata('Sales'), columns: [{ metadata: metadata('Quantity'), kind: 'data', data_type: 'int64', source_column: 'Quantity', expression: null, format_string: null, sort_by_column: null }],
      measures: [{ metadata: metadata('Sales Amount'), expression, format_string: '$#,0', display_folder: 'Revenue' }], hierarchies: [],
      partitions: [{ metadata: metadata('ImportSales'), source_kind: 'm', mode: 'import', expression: { ...expression, display: 'let\n    x = 1\nin x' } }] }],
    relationships: [{ metadata: metadata('SalesDate'), from: { table: 'Sales', object: 'OrderDateKey', resolved: true }, to: { table: 'Date', object: 'DateKey', resolved: true }, from_cardinality: 'many', to_cardinality: 'one', cross_filter: 'oneDirection', is_active: true, security_filter: null }],
    roles: [], perspectives: [], cultures: [], expressions: [], functions: [],
  },
}
afterEach(cleanup)
function select(name: string) {
  render(<SemanticExplorer inspection={inspection} />)
  // Open the table's disclosure exactly as a user would.
  fireEvent.click(screen.getByText('Objects in Sales'))
  fireEvent.click(screen.getByRole('button', { name }))
  return within(screen.getByRole('region', { name: 'Read-only object inspector' }))
}
describe('semantic inspection', () => {
  it('renders the semantic tree and all object groups', () => {
    render(<SemanticExplorer inspection={inspection} />)
    for (const name of ['Tables', 'Relationships', 'Roles', 'Perspectives', 'Cultures', 'Expressions', 'Functions']) expect(screen.getAllByText(name).length).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: 'Table: Sales' })).toBeTruthy()
  })
  it('inspects a table', () => { expect(select('Table: Sales').getByText('Table · Read-only')).toBeTruthy() })
  it('inspects a column', () => { expect(select('Column: Quantity').getByText('int64')).toBeTruthy() })
  it('displays an opaque measure expression and metadata', () => {
    const inspector = select('Measure: Sales Amount')
    expect(inspector.getByLabelText('Expression').textContent).toBe('SUM(Sales[Quantity])\n')
    expect(inspector.getByText('$#,0')).toBeTruthy()
    expect(inspector.getByText('Revenue')).toBeTruthy()
    expect(inspector.queryByRole('textbox')).toBeNull()
  })
  it('inspects relationship endpoints and active status', () => {
    const inspector = select('Relationship: SalesDate')
    expect(inspector.getByText(/Sales\[OrderDateKey\].*Date\[DateKey\]/)).toBeTruthy()
    expect(inspector.getByText('oneDirection')).toBeTruthy()
  })
  it('displays partition source and opaque M', () => {
    const inspector = select('Partition: ImportSales')
    expect(inspector.getByText('Power Query M')).toBeTruthy()
    expect(inspector.getByLabelText('Expression').textContent).toContain('in x')
  })
  it('renders source-located diagnostics and partial state', () => {
    render(<SemanticExplorer inspection={{ ...inspection, status: 'partial', diagnostics: [{ severity: 'error', code: 'TMDL_SYNTAX_ERROR', message: 'Invalid indentation', source: span }] }} />)
    expect(screen.getByText('TMDL_SYNTAX_ERROR')).toBeTruthy()
    expect(screen.getByRole('status').textContent).toContain('Partial inspection')
    expect(screen.getByText(/tables\/Sales.tmdl:3:1/)).toBeTruthy()
  })
  it('explains unsupported TMSL inspection without blocking project discovery', () => {
    render(<SemanticExplorer inspection={{ status: 'unsupported', model: null, diagnostics: [] }} />)
    expect(screen.getByText(/Not yet supported for this format/)).toBeTruthy()
    expect(screen.queryByRole('navigation')).toBeNull()
  })
})
