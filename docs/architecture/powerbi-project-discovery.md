# Read-only Power BI project discovery

## Scope

WP01 inspects the outer project envelope. Format labels describe structural
markers, not validation of report visuals, TMDL grammar, DAX, or model semantics.
No project editing, saving, migration, remote connection, or publishing occurs.

## Selection and command boundary

React calls `select_powerbi_project`. Rust uses the Tauri dialog plugin to select
one local `.pbip` file. Cancellation returns `null` and preserves the current UI
summary. Non-UTF-8 selected paths return an explicit error rather than a lossy
path that might identify a different file.

React then calls `open_powerbi_project` with the selected path. The command runs
`biforgeworks_powerbi::discover_project` on a blocking worker and returns its
typed `PowerBiProjectSummary`. React receives only project/component paths,
existence flags, format enums, and diagnostics. It receives no raw file contents.

The native dialog is configured in Rust. General dialog and filesystem plugin
permissions are not granted to JavaScript. No shell, arbitrary file-read,
write/save, or remote-resource loading command is exposed.

## Reference chain

1. The selected `.pbip` parent directory defines the project root.
2. `artifacts[].report.path` identifies the report relative to that root.
3. `definition.pbir` in the report identifies the semantic model via
   `datasetReference.byPath.path`, relative to the report directory.
4. A sibling reference such as `../Sales.SemanticModel` is valid when the
   resolved target stays inside the project root.

Remote `byConnection` references are outside WP01. Connection strings are never
returned or executed. Discovery does not fetch `$schema` URLs.

## Format markers

| Component | Format | Structural markers |
| --- | --- | --- |
| Report | PBIR | `definition.pbir`, `definition/report.json`, `definition/version.json` |
| Report | PBIR-Legacy | `definition.pbir`, `report.json` |
| Semantic model | TMDL | `definition.pbism`, `definition/model.tmdl` |
| Semantic model | TMSL | `definition.pbism`, `model.bim` |

Component folder names are not evidence of a format. Missing components and
unrecognized or ambiguous layouts have explicit `MISSING`/`UNKNOWN` results and
diagnostics. Validity is limited to the outer structure supported by WP01.

WP01 recognizes PBIP envelope versions `1.x`, item definition versions `1.0`
and `4.x`, and PBIR version metadata `1.x.0`/`2.x.0`. Unknown schema families or
major versions produce diagnostics rather than a claim of validity. A missing
`$schema` is diagnosed but does not prevent inspecting known structural markers.
Multiple report references are ambiguous; WP01 does not choose one silently.

## Diagnostics

Every diagnostic has severity, stable code, a human-readable message, and an
optional local path. Malformed input is returned as diagnostics rather than
panics or raw JSON. The UI displays these as text. Paths remain local and are not
sent to an external service.

## Read-only and security boundaries

All project access is read-only and bounded. Referenced paths must remain under
the project root. Unsafe links, nonregular metadata, invalid encoding, and
oversized metadata must be diagnosed rather than followed, decoded lossily, or
read without limit. Linux reads preserve access timestamps; inability to provide
that guarantee fails closed. No temporary files, backups, normalizations, or
timestamp-restoration writes are made inside the selected project.

The Linux implementation holds directory descriptors and resolves each project
path segment with `openat`/`fstatat` without following symlinks. It does not list
directories. Metadata reads use `O_NOATIME`, require regular files, and read at
most 1 MiB per envelope. References are limited to 1,024 bytes, 255 bytes per
segment, and 32 segments. Duplicate JSON keys and excessive nesting are rejected;
a leading UTF-8 BOM is accepted. Error messages do not quote metadata contents.

The user-selected root may itself be reached through a symlink (for example a
symlinked development directory); references below the opened root cannot follow
symlinks. Displayed paths are local labels, not general filesystem capabilities.
Discovery is Linux-only for now: other platforms return
`READ_ONLY_GUARANTEE_UNAVAILABLE` rather than silently weakening preservation.
Files owned by another user may also return that diagnostic when the kernel
refuses `O_NOATIME`. This is not a filesystem snapshot: concurrent external edits
can affect which structural markers are observed.

This boundary is not an operating-system sandbox against privileged filesystem
changes: bind mounts are not rejected, and the checks do not defend against a
privileged process replacing a regular file with a device during an open.

The regression suite exercises synthetic fixtures and checks project bytes and
filesystem metadata before and after discovery. Test setup may construct data
in temporary directories; it is separate from production discovery.

## Authoritative format references

- [Microsoft PBIP overview](https://learn.microsoft.com/en-us/power-bi/developer/projects/projects-overview)
- [Microsoft project report folder](https://learn.microsoft.com/en-us/power-bi/developer/projects/projects-report)
- [Microsoft project semantic-model folder](https://learn.microsoft.com/en-us/power-bi/developer/projects/projects-dataset)
- [Microsoft JSON schemas](https://github.com/microsoft/json-schemas/tree/main/fabric)
- [Tauri native dialogs](https://v2.tauri.app/plugin/dialog/)

These references informed discovery; they are not downloaded when opening a project.
