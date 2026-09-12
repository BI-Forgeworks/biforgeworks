# TMDL reader

WP03 adds read-only semantic inspection inside `biforgeworks-powerbi::tmdl`.
No TMDL writer, expression evaluator, or semantic edit operation is exposed.

## Pipeline and API

```text
selected PBIP → existing discovery → confined definition/ reader
      → line/token scanner → source syntax → typed semantic projection
      → inspect_powerbi_semantic_model → React explorer / inspector
```

`inspect_project(&Path)` uses WP01 discovery to identify the local TMDL component.
`inspect_sources(project_identity, sources)` provides the same parser/projection
without filesystem access. `parse_document` exposes individual source syntax.
The new Tauri command accepts no path: it inspects the backend's currently opened
project on a blocking worker. Opening/cancelling selection retains existing WP01
behavior. TMSL discovery remains available, but semantic inspection is unsupported.

React renders typed DTOs and text, including opaque DAX/M. It receives no general
filesystem interface. Original syntax documents stay in the Rust inspection result
and are omitted from serialization; each projected expression retains its exact
raw source and its display form. There is no write bridge to WP02.

## Syntax and source locations

The parser scans structural lines and quote-aware identifier/property tokens,
then builds an indentation tree. It does not use regular expressions. Expressions
are scanned as opaque regions, so their internal punctuation and keywords never
become TMDL declarations. It supports tabs or a consistent inferred space unit;
mixed indentation within one structural prefix is rejected rather than guessed.

Every document retains its full UTF-8 text, including BOM, newline style, unknown
constructs, and whitespace. Spans use end-exclusive UTF-8 byte offsets plus
one-based line and character column. A BOM does not count as a visible column.
Object spans can include trailing separator whitespace; sibling spans are disjoint.
Descriptions retain their text in the projection and their original spelling in
the source document. Future editors must use operation-specific spans, not
serialize the reduced semantic model or assume an object span is a safe edit.

`Expression.raw` is an exact source slice without fences. `display` removes only
structural indentation (or the literal closing-fence prefix) for inspection.
Trailing whitespace/newlines remain available; neither form is a DAX/M AST.
Unknown properties and blocks remain in source syntax. No expression is executed.

Invalid syntax produces source-located errors. A document with syntax errors is
excluded from semantic projection while its original text and partial syntax are
retained. Other valid documents can still be inspected with a conspicuous Partial
status. Unsupported constructs produce warnings, not silent deletion or a claim
of complete TOM validation.

## Semantic projection and identity

Power BI-specific types cover database/model metadata, tables, three common column
forms, measures, ordered hierarchies, partitions, relationships, roles/permissions/
members, perspectives, cultures, named expressions, and opaque functions.
Known metadata and annotations are inspectable. Culture translation records expose
source targets/properties; linguistic JSON/XML remains opaque.

Implicit model children are assembled across documents. Partial declarations
combine distinct properties and retain contributing source locations; repeated
properties or expressions generate duplicate diagnostics. `ref` entries establish
collection order, with remaining definitions appended. Dangling relationships,
hierarchy columns, sort columns, role tables, and perspective members are diagnosed.
This is reference inspection, not full TOM type/enum/semantic validation.

IDs are SHA-256 over length-delimited project scope, object kind, and lineage tag
when present, otherwise the object name. Children use their parent's ID as scope.
They do not depend on array position or source line. Without lineage, renaming an
object changes its ID. The filesystem entry point uses the selected absolute PBIP
path as project scope, so moving a project changes its fallback identity scope.
No cross-machine identity or dependency graph is promised by WP03.

## Diagnostics

`TmdlDiagnostic` is a separate typed diagnostic carrying a source span; WP01's
outer-project diagnostic wire format remains unchanged. Codes include
`TMDL_SYNTAX_ERROR`, `TMDL_UNKNOWN_PROPERTY`, `TMDL_DUPLICATE_OBJECT`,
`TMDL_UNRESOLVED_REFERENCE`, `TMDL_INVALID_RELATIONSHIP_REFERENCE`,
`TMDL_INVALID_HIERARCHY_REFERENCE`, `TMDL_UNSUPPORTED_OBJECT`,
`TMDL_READ_FAILED`, `TMDL_LIMIT_EXCEEDED`, and `TMDL_UNSUPPORTED_FORMAT`.
Messages do not echo source excerpts or untrusted values; source locations identify
the problem. Source expressions may themselves contain sensitive information and
are displayed only locally, never logged or sent to a remote service.

## Filesystem and resource limits

Only the discovered model's `definition/` tree is read. TMDLScripts, DAXQueries,
caches, and report visuals are not parsed. Optional files/subfolders may be absent;
unusual filenames are supported because content determines objects.

Linux uses existing descriptor-relative no-follow/no-atime operations for bounded
directory enumeration and regular-file reads. Unsafe links/special entries,
invalid UTF-8, inaccessible files, and observed changes during reading fail closed.
No source writes, temporary files, timestamp restoration, or snapshot category
changes occur. This is a sequential read, not an atomic external-filesystem
snapshot; the existing bind-mount limitation remains.

Limits: 8 MiB per source, 64 MiB total, 2,048 source files, 20,000 directory entries,
16 nested directories, 64 syntax levels, and 100,000 syntax nodes per inspection.
Very large or inaccessible models can be rejected. The parser is portable over
in-memory strings; timestamp-preserving filesystem inspection remains Linux-only.

## Compatibility research

Microsoft's [TMDL overview](https://learn.microsoft.com/en-us/analysis-services/tmdl/tmdl-overview)
informed quoting, indentation, opaque/fenced expressions, partial declarations,
descriptions, and reference ordering. The
[object reference](https://learn.microsoft.com/en-us/analysis-services/tmdl/tmdl-reference-tabular-object)
describes virtual translation hierarchies and role members. The
[PBIP semantic-model layout](https://learn.microsoft.com/en-us/power-bi/developer/projects/projects-dataset)
separates definitions from editor scripts and caches. The
[DAX UDF documentation](https://learn.microsoft.com/en-us/dax/best-practices/dax-user-defined-functions)
identifies functions.tmdl; bodies remain opaque here. References were checked for
WP03, not fetched when opening projects.

Official examples are not a complete grammar oracle. In particular, dotted
function names are accepted conservatively, anonymous database declarations are
supported, and property `=` forms seen in examples are retained. These choices
need real serializer/Power BI Desktop corpus validation before semantic writes.

## Validation and next boundary

Synthetic fixtures cover objects, quoting, multiline/fenced text, source integrity,
unknown syntax, invalid inputs, security, and read-only preservation. The moderate
fixture contains 50 tables, 500 columns, 250 measures, and 100 relationships.
See [WP03 validation](../work-packages/WP03-validation.md) for measured results.

No approved Desktop-generated sample was available during WP03. **Real Power BI
Desktop-created TMDL PBIP validation is a hard prerequisite before WP04 production
semantic writes.** Synthetic success does not establish Desktop compatibility.
Calculation groups and other unsupported TOM objects remain source-preserved with
warnings; no TMDL, DAX, M, or PBIR semantic writes are enabled.

The lexical input is additionally limited to 200,000 newlines per source; the
aggregate syntax-node budget is checked as each document is parsed.
