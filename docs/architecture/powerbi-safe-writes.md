# Power BI safe writes

WP02 adds a Rust persistence foundation inside `biforgeworks-powerbi`. The
desktop continues to expose read-only discovery. There is no general save
button, filesystem write command, or semantic editor.

## Operation boundary

The one production mutation changes an **existing** boolean at
`settings.enableAutoRecovery` in the selected PBIP. Microsoft's
[PBIP properties schema](https://github.com/microsoft/json-schemas/blob/main/fabric/pbip/pbipProperties/1.0.0/schema.json)
defines this as the Power BI Desktop recovery-file setting. This is project
metadata, not report or model semantics.

Only that boolean token is replaced. The operation must preserve surrounding
whitespace, JSON property ordering, unknown properties, UTF-8 BOM state, and
trailing newline behavior. Missing settings are rejected rather than inserted.
Managed JSON is validated before source replacement. Files outside the explicit
change set are never serialized or normalized.

A managed replacement receives a new inode and write timestamps. Creating and
removing recovery or temporary entries also changes their parent directories'
bookkeeping. Rollback restores managed content, but cannot restore the original
inode or its change time. These differences are distinct from preservation of
unrelated regular files, whose bytes and metadata must remain untouched. A no-op
has no such exceptions: it creates no artifacts at all.

## Snapshot and preservation contract

A source snapshot covers the selected project root, including unknown files and
opaque directories. Hashes determine content changes; file identity and metadata
also detect replacement and type changes. Reads must retain WP01's no-follow and
no-access-time-update behavior. Projects that cannot be safely inspected fail
closed instead of receiving a weaker snapshot.

`ProjectSnapshot` records a root identity, capture time, sorted relative entries,
file/directory counts, and total bytes. Each `SnapshotEntry` records its category,
type, device/inode identity, owner/group, permissions, link count, size, modification
and change times, and SHA-256 for regular files. It is a sequential observation, not an atomic
filesystem snapshot. Symlinks and special nodes are inspected through metadata
only, without opening them or following their targets.

Capture is bounded to 20,000 entries, 32 nested directory levels, 256 MiB per
regular file, and 512 MiB of total regular-file content. File hashing is streamed.
Non-UTF-8 entry names and unreadable regular files fail the capture. These limits
are deliberate WP02 restrictions and may exclude large real projects.

Hashing uses [RustCrypto `sha2`](https://github.com/RustCrypto/hashes/tree/master/sha2)
(`0.10.9` in the lockfile), an existing transitive dependency now used directly.
It provides incremental SHA-256 under MIT/Apache-2.0 licensing; Rust's standard
library does not provide this content hash. WP02 adds no new frontend dependency.

The selected PBIP is managed only for the authorized metadata operation. Other
recognized project content is preserved. Unrecognized content is unknown and
receives the same preservation guarantee. A no-op must create no transaction
artifacts and alter no source content or metadata.

Before saving, compare the entire project with the opening snapshot. A change,
addition, deletion, or replacement in preserved or unknown content also blocks
the transaction. WP02 does not auto-merge or silently refresh away a conflict.

## Transaction sequence

```text
open and snapshot
       ↓
stage explicit operation → Dirty
       ↓
acquire same-project save lock → Saving
       ↓
validate staged bytes and source snapshot
       ↓
retain originals and prepare complete replacement bytes
       ↓
flush and atomically replace each managed file
       ↓
rediscover project and verify formats/references
       ↓
clean up recovery artifacts and refresh snapshot → Clean
```

Failures produce typed diagnostics and a Conflict or Error state. If replacements
have begun, rollback attempts to restore originals. Recovery evidence must remain
when restoration cannot safely finish. State belongs to Rust, not React.

## Rust API

The public Linux API is `ProjectSession::open`, `snapshot`, `summary`, `state`,
`begin_transaction`, and explicit `refresh`. A transaction supports
`stage_auto_recovery`, `staged`, `validate`, `commit`, and `cancel`. Staging is
in-memory; `validate` checks staged destinations/content, while `commit` also
performs mandatory source conflict checks. Dropping or cancelling staged edits
discards them. A consumed failed commit does not leave an inaccessible dirty
payload in the session.

`CommitReceipt` returns changed paths, a no-op indicator, rediscovery result,
and fresh snapshot. `SafeWriteError` contains a typed kind and stable diagnostics.
Examples include `EXTERNAL_FILE_CHANGED`, `EXTERNAL_FILE_REMOVED`,
`EXTERNAL_FILE_REPLACED`, `UNSAFE_WRITE_TARGET`, `PROJECT_NOT_SAVEABLE`,
`CONCURRENT_SAVE_BLOCKED`, `WRITE_FAILED`, `POST_SAVE_VALIDATION_FAILED`,
`SAVE_ROLLED_BACK`, `ROLLBACK_FAILED`, and `RECOVERY_REQUIRED`.

## Failure model

Atomic replacement is a **per-file** guarantee on supported local Linux
filesystems. Readers may see intermediate states during a multi-file commit.
There is no claim of a filesystem-wide transaction, distributed locking, or
unconditional power-loss safety on every filesystem or storage device.
Linux documents atomic destination replacement in
[`rename(2)`](https://man7.org/linux/man-pages/man2/rename.2.html); durable directory
entries also require directory synchronization, as described in
[`fsync(2)`](https://man7.org/linux/man-pages/man2/fsync.2.html).

The same-process lock prevents concurrent BI Forgeworks commits for the same
physical root. It does not lock another application. Snapshot comparisons detect
external changes; an uncooperative external writer can still race filesystem
operations. Recovery must not knowingly overwrite a newer external edit.

## Recovery

During a save, `.biforgeworks-save/` holds `journal.json` and numbered original
files under `originals/`. Recovery directories use owner-only access; original
copies use mode `0600`. A later session detects a retained journal, reports it
through `pending_recovery()`, and refuses transactions. It never replays a
journal or deletes retained artifacts automatically.

Manual recovery is deliberately outside the write API in WP02:

1. Stop other tools from editing the project and preserve a copy of the whole
   project, including recovery artifacts, outside the source directory.
2. Inspect the journal and current files. Treat journal paths as untrusted input;
   validate containment and never follow symlinks or execute embedded content.
3. Compare each available original's SHA-256 with the journal and compare current
   target content with both original and replacement hashes. The `replaced` flag
   records replacement intent before rename; it is not proof that rename happened.
4. Resolve differences explicitly, preserving any later external work. A missing
   or malformed journal requires investigation, not guessed restoration.
5. After confirming a valid project, remove only reviewed transaction artifacts
   and reopen the session. Do not use a wildcard cleanup of similarly named files.

A crash may leave a sibling `.biforgeworks-tmp-*` file as well as the journal.
Its presence does not grant permission to delete another tool's content. Cleanup
failure after a validated commit is distinct from rollback failure: saved content
may already be final, while some cleanup evidence remains.

## Security boundary

All production targets derive from the selected project metadata. React cannot
provide an arbitrary destination and bytes. Reject traversal, absolute relative
destinations, unsafe symlinks, unsupported file types, and destinations outside
the selected root. Use descriptor-relative operations to retain path confinement.
Never execute project content, fetch remote resources, or load custom visuals.

Managed files with multiple hard links, a different owner, extended attributes,
or ACLs are rejected rather than replaced with reduced metadata. Replacement
files must retain the original owner/group and permissions. Opening through an
explicitly selected root alias is supported, but the root's physical identity
must continue to match that selected path. Read-only discovery remains available
for projects that the write layer cannot safely save.

Bind mounts inside the selected directory are treated as part of its tree; WP02
does not implement a separate mount-boundary policy. The current production
operation only replaces the selected root-level PBIP. Future operations that
write inside component directories must review this limitation.

## Scope

No TMDL semantic parser, DAX editor, PBIR semantic parser or visual editor,
authentication, Fabric publishing, or BFIR writer is introduced. Future editors
must add explicit operations and operation-specific validation through this
boundary. See [ADR-0005](../adr/0005-snapshot-based-explicit-transactions.md).
