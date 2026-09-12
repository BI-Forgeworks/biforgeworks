import { useState, type ReactNode } from 'react'
import type { Expression, ObjectMetadata, SemanticInspection, SemanticModel } from './api/semanticModel'

interface Item { metadata: ObjectMetadata; type: string; details: ReactNode }
function Fields({ values }: { values: Record<string, string | number | boolean | null> }) {
  return <dl>{Object.entries(values).map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value === null ? 'Not specified' : typeof value === 'boolean' ? (value ? 'Yes' : 'No') : value}</dd></div>)}</dl>
}
function SourceExpression({ expression, label = 'Expression' }: { expression: Expression | null; label?: string }) {
  return expression ? <section><h4>{label}</h4><pre aria-label={label}>{expression.display}</pre><details><summary>Exact expression source</summary><pre>{expression.raw}</pre></details></section> : null
}
function Metadata({ metadata }: { metadata: ObjectMetadata }) {
  return <>
    {metadata.description && <p>{metadata.description}</p>}
    <Fields values={{ Hidden: metadata.is_hidden, 'Lineage tag': metadata.lineage_tag }} />
    {metadata.properties.length > 0 && <details><summary>Properties</summary><dl>{metadata.properties.map((p, i) => <div key={`${p.name}:${i}`}><dt>{p.name}</dt><dd>{p.value}</dd></div>)}</dl></details>}
    {metadata.annotations.length > 0 && <details><summary>Annotations</summary><dl>{metadata.annotations.map((p, i) => <div key={`${p.name}:${i}`}><dt>{p.name}</dt><dd>{p.value}</dd></div>)}</dl></details>}
    <h4>Source</h4>{metadata.sources.map(s => <p key={`${s.file}:${s.start}`}>{s.file}:{s.line}:{s.column} · bytes {s.start}–{s.end}</p>)}
  </>
}

function ModelExplorer({ model }: { model: SemanticModel }) {
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [expandedTables, setExpandedTables] = useState<Set<string>>(new Set())
  const items = new Map<string, Item>()
  const select = (metadata: ObjectMetadata, type: string, details: ReactNode = null) => {
    items.set(metadata.id, { metadata, type, details })
    return <button type="button" aria-label={`${type}: ${metadata.name}`} aria-pressed={selectedId === metadata.id} onClick={() => setSelectedId(metadata.id)}>{metadata.name}</button>
  }
  const group = (name: string, content: ReactNode) => <details open><summary>{name}</summary><ul>{content}</ul></details>
  const navigation = <nav aria-label="Semantic model explorer">
      {select(model.metadata, 'Model')}
      {model.database && select(model.database, 'Database')}
      {group('Tables', model.tables.map(t => <li key={t.metadata.id}>
        {select(t.metadata, 'Table', <Fields values={{ Columns: t.columns.length, Measures: t.measures.length, Hierarchies: t.hierarchies.length, Partitions: t.partitions.length }} />)}
        <button type="button" aria-expanded={expandedTables.has(t.metadata.id)} onClick={() => setExpandedTables(previous => {
          const next = new Set(previous)
          if (next.has(t.metadata.id)) next.delete(t.metadata.id)
          else next.add(t.metadata.id)
          return next
        })}>Objects in {t.metadata.name}</button>
        {expandedTables.has(t.metadata.id) && <div>
          {group('Columns', t.columns.map(c => <li key={c.metadata.id}>{select(c.metadata, 'Column', <><Fields values={{ Table: t.metadata.name, Kind: c.kind, 'Data type': c.data_type, 'Source column': c.source_column, Format: c.format_string, 'Sort by column': c.sort_by_column }} /><SourceExpression expression={c.expression} /></>)}</li>))}
          {group('Measures', t.measures.map(m => <li key={m.metadata.id}>{select(m.metadata, 'Measure', <><Fields values={{ Table: t.metadata.name, Format: m.format_string, 'Display folder': m.display_folder }} /><SourceExpression expression={m.expression} /></>)}</li>))}
          {group('Hierarchies', t.hierarchies.map(h => <li key={h.metadata.id}>{select(h.metadata, 'Hierarchy', <ol>{h.levels.map(l => <li key={l.metadata.id}>{l.metadata.name} → {l.column ?? 'Unresolved'} (ordinal {l.ordinal})</li>)}</ol>)}</li>))}
          {group('Partitions', t.partitions.map(p => <li key={p.metadata.id}>{select(p.metadata, 'Partition', <><Fields values={{ Mode: p.mode, Source: p.source_kind === 'm' ? 'Power Query M' : p.source_kind }} /><SourceExpression expression={p.expression} /></>)}</li>))}
        </div>}
      </li>))}
      {group('Relationships', model.relationships.map(r => <li key={r.metadata.id}>{select(r.metadata, 'Relationship', <><p className="relationship-endpoints">{r.from ? `${r.from.table}[${r.from.object}]` : 'Unresolved'} ({r.from_cardinality}) → {r.to ? `${r.to.table}[${r.to.object}]` : 'Unresolved'} ({r.to_cardinality})</p><Fields values={{ Active: r.is_active, 'Cross-filter': r.cross_filter, 'Security filter': r.security_filter, 'References resolved': (r.from?.resolved ?? false) && (r.to?.resolved ?? false) }} /></>)}</li>))}
      {group('Roles', model.roles.map(r => <li key={r.metadata.id}>{select(r.metadata, 'Role', <><Fields values={{ Permission: r.model_permission }} />{r.filters.map(f => <section key={f.metadata.id}><h4>{f.table}</h4><SourceExpression expression={f.expression} label="Filter expression" /><Metadata metadata={f.metadata} />{f.column_permissions.map(c => <p key={c.metadata.id}>{c.metadata.name}: {c.permission}</p>)}</section>)}<h4>Members</h4>{r.members.map(m => <p key={m.metadata.id}>{m.metadata.name} · {m.member_type ?? 'Default'} · {m.identity_provider ?? 'Default provider'}</p>)}</>)}</li>))}
      {group('Perspectives', model.perspectives.map(p => <li key={p.metadata.id}>{select(p.metadata, 'Perspective', <ul>{p.tables.map(t => <li key={t.metadata.id}>{t.table}<Fields values={{ Columns: t.columns.join(', '), Measures: t.measures.join(', '), Hierarchies: t.hierarchies.join(', '), Resolved: t.resolved }} /></li>)}</ul>)}</li>))}
      {group('Cultures', model.cultures.map(c => <li key={c.metadata.id}>{select(c.metadata, 'Culture', <><p>{c.translations.length} translation records</p>{c.translations.map((t, i) => <section key={i}><h4>{t.target}</h4>{t.properties.map(p => <p key={p.name}>{p.name}: {p.value}</p>)}</section>)}<SourceExpression expression={c.linguistic_metadata} label="Linguistic metadata" /></>)}</li>))}
      {group('Expressions', model.expressions.map(e => <li key={e.metadata.id}>{select(e.metadata, 'Named expression', <><Fields values={{ Kind: e.kind }} /><SourceExpression expression={e.expression} /></>)}</li>))}
      {group('Functions', model.functions.map(f => <li key={f.metadata.id}>{select(f.metadata, 'Function', <SourceExpression expression={f.expression} />)}</li>))}
    </nav>
  const selected = selectedId ? items.get(selectedId) : undefined
  return <div className="semantic-layout">
    {navigation}
    <section aria-label="Read-only object inspector" className="semantic-inspector">
      {selected ? <><h3>{selected.metadata.name}</h3><p>{selected.type} · Read-only</p>{selected.details}<Metadata metadata={selected.metadata} /></> : <><h3>Semantic model summary</h3><Fields values={{ Tables: model.tables.length, Columns: model.tables.reduce((n, t) => n + t.columns.length, 0), Measures: model.tables.reduce((n, t) => n + t.measures.length, 0), Relationships: model.relationships.length, Roles: model.roles.length, Perspectives: model.perspectives.length }} /><p>Select an object to inspect its properties and source.</p></>}
    </section>
  </div>
}

export function SemanticExplorer({ inspection }: { inspection: SemanticInspection }) {
  return <section aria-label="Semantic inspection" className="semantic-explorer">
    <h2>Semantic Model · Read-only inspection</h2>
    {inspection.status === 'unsupported' && <p>Semantic inspection: Not yet supported for this format.</p>}
    {inspection.status === 'partial' && <p role="status">Partial inspection: some source could not be parsed reliably.</p>}
    {inspection.diagnostics.length > 0 && <details open><summary>Semantic diagnostics ({inspection.diagnostics.length})</summary><ul>{inspection.diagnostics.map((d, i) => <li key={i} className={`semantic-diagnostic--${d.severity}`}><strong>{d.code}</strong>: {d.message}{d.source && <span> — {d.source.file}:{d.source.line}:{d.source.column}</span>}</li>)}</ul></details>}
    {inspection.model && <ModelExplorer model={inspection.model} />}
  </section>
}
