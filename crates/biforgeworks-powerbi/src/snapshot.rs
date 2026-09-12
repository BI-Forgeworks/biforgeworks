//! Whole-tree project snapshots.
//!
//! A snapshot records every entry below the project root — including files
//! and directories this crate knows nothing about — with a content hash as
//! the authoritative record plus the identity and metadata needed to notice
//! replacement, deletion, type changes, and ownership or permission
//! changes. Nothing is excluded except entries a transaction created
//! itself, named exactly.
//!
//! A snapshot is a bounded *observation*, not an atomic instant: the tree is
//! walked entry by entry, so a project being edited concurrently can be
//! observed part-way through that edit. Two things make that safe rather
//! than silent. Each file's metadata is re-checked after its bytes are
//! hashed, so a file rewritten mid-read is reported as unstable instead of
//! recorded; and a commit re-observes the whole tree, then re-checks each
//! target again immediately before replacing it, so anything that shifted in
//! between becomes a conflict.
//!
//! Capture never follows symlinks, never updates access times, and fails
//! closed: a file that cannot be read or hashed aborts the snapshot rather
//! than being silently omitted. Symlinks and special files (FIFOs, sockets,
//! devices) are legitimate unknown project content, so they are tracked from
//! `lstat` metadata alone — type, identity, mode, ownership, link count —
//! and never opened, read, or resolved. A change to one is still detected
//! through its identity or metadata.

use crate::fs_linux::{Dir, EntryKind, FileStat, FsError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

/// Most entries (files plus directories) a snapshot will hold.
pub const MAX_SNAPSHOT_ENTRIES: usize = 20_000;
/// Deepest directory nesting a snapshot will walk, below the root.
pub const MAX_SNAPSHOT_DEPTH: usize = 32;
/// Largest total byte count a snapshot will hash.
pub const MAX_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
/// Largest single file a snapshot will hash.
pub const MAX_SNAPSHOT_FILE_BYTES: u64 = 256 * 1024 * 1024;

const HASH_CHUNK: usize = 64 * 1024;

/// What a project file means to this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileCategory {
    /// May be modified by a transaction (currently only the `.pbip`).
    Managed,
    /// Discovery depends on it; it must survive a save byte for byte.
    Preserved,
    /// Everything else, including files this crate does not understand.
    Unknown,
}

/// Entry type as recorded by `lstat`; symlinks and special files are tracked
/// but never opened or followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntryType {
    File,
    Directory,
    Symlink,
    Special,
}

/// Filesystem identity, which distinguishes an edited file from a replaced
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
}

/// One recorded entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotEntry {
    /// Path relative to the project root, `/`-separated.
    pub path: String,
    pub entry_type: EntryType,
    pub category: FileCategory,
    pub identity: FileIdentity,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub link_count: u64,
    pub size: u64,
    pub mtime_secs: i64,
    pub mtime_nanos: i64,
    pub ctime_secs: i64,
    pub ctime_nanos: i64,
    /// Lowercase hex SHA-256 of the file's bytes; `None` for anything that
    /// is not a regular file.
    pub content_sha256: Option<String>,
}

impl SnapshotEntry {
    /// Metadata that must match for an entry to count as unchanged, content
    /// aside: type, identity, permissions, ownership, and link count.
    ///
    /// For a regular file this also covers modification and change times,
    /// which are supplemental to the hash: they catch a same-content rewrite
    /// onto a reused inode, or a metadata-only change, that comparing bytes
    /// alone would miss. (Access time is deliberately not compared: reading
    /// the project must never be what makes it look changed.)
    ///
    /// Timestamps and size are not compared for directories: both move
    /// merely because entries came and went — including a transaction's own
    /// temporary files, which live in the directory they replace into — and
    /// directory changes are already reported precisely as added and removed
    /// entries.
    fn metadata_matches(&self, other: &Self) -> bool {
        let timestamps = self.entry_type != EntryType::File
            || (self.mtime_secs == other.mtime_secs
                && self.mtime_nanos == other.mtime_nanos
                && self.ctime_secs == other.ctime_secs
                && self.ctime_nanos == other.ctime_nanos);
        self.entry_type == other.entry_type
            && self.identity == other.identity
            && self.mode == other.mode
            && self.uid == other.uid
            && self.gid == other.gid
            && self.link_count == other.link_count
            && timestamps
    }
}

/// A bounded observation of the whole project tree (see the module
/// documentation for what "bounded observation" rules out).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectSnapshot {
    pub root: String,
    pub root_identity: FileIdentity,
    pub captured_at_unix_millis: u128,
    pub entries: Vec<SnapshotEntry>,
    pub file_count: usize,
    pub directory_count: usize,
    pub total_bytes: u64,
}

impl ProjectSnapshot {
    pub fn entry(&self, path: &str) -> Option<&SnapshotEntry> {
        self.entries
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Compares two snapshots of the same project, ignoring the paths in
    /// `ignore` (used only for entries the caller created itself).
    ///
    /// Content is compared for regular files only. Symlinks and special
    /// files are compared by identity and metadata, since reading them is
    /// refused; directories are compared by identity, metadata, and the
    /// entries they gained or lost.
    pub fn diff(&self, later: &ProjectSnapshot, ignore: &BTreeSet<String>) -> SnapshotDiff {
        let mut changes = Vec::new();
        if self.root_identity != later.root_identity {
            changes.push(EntryChange {
                path: String::new(),
                change: ChangeKind::RootReplaced,
            });
        }

        let before: BTreeMap<&str, &SnapshotEntry> = self
            .entries
            .iter()
            .filter(|entry| !ignore.contains(&entry.path))
            .map(|entry| (entry.path.as_str(), entry))
            .collect();
        let after: BTreeMap<&str, &SnapshotEntry> = later
            .entries
            .iter()
            .filter(|entry| !ignore.contains(&entry.path))
            .map(|entry| (entry.path.as_str(), entry))
            .collect();

        for (path, old) in &before {
            match after.get(path) {
                None => changes.push(EntryChange {
                    path: (*path).to_owned(),
                    change: ChangeKind::Removed,
                }),
                Some(new) => {
                    let change = if old.entry_type != new.entry_type {
                        Some(ChangeKind::TypeChanged)
                    } else if old.identity != new.identity {
                        Some(ChangeKind::Replaced)
                    } else if old.content_sha256 != new.content_sha256
                        || (old.entry_type == EntryType::File && old.size != new.size)
                    {
                        Some(ChangeKind::ContentModified)
                    } else if !old.metadata_matches(new) {
                        Some(ChangeKind::MetadataChanged)
                    } else {
                        None
                    };
                    if let Some(change) = change {
                        changes.push(EntryChange {
                            path: (*path).to_owned(),
                            change,
                        });
                    }
                }
            }
        }
        for path in after.keys() {
            if !before.contains_key(path) {
                changes.push(EntryChange {
                    path: (*path).to_owned(),
                    change: ChangeKind::Added,
                });
            }
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        SnapshotDiff { changes }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChangeKind {
    Added,
    Removed,
    /// Same inode, different bytes.
    ContentModified,
    /// Same path, different inode: the file was replaced.
    Replaced,
    /// File became a directory, symlink, or special file, or the reverse.
    TypeChanged,
    /// Permissions, ownership, or hard-link count changed.
    MetadataChanged,
    /// The project root directory itself is not the one first opened.
    RootReplaced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryChange {
    pub path: String,
    pub change: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotDiff {
    pub changes: Vec<EntryChange>,
}

impl SnapshotDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Why a snapshot could not be taken. Capture never partially succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SnapshotError {
    /// An entry could not be read, so preservation could not be verified.
    Unreadable {
        path: String,
        error: FsError,
    },
    /// A file changed while it was being hashed.
    Unstable {
        path: String,
    },
    TooManyEntries,
    TooDeep {
        path: String,
    },
    TooLarge {
        path: String,
    },
    /// A name that cannot be represented as a relative UTF-8 path.
    UnsupportedName {
        path: String,
    },
}

/// Decides each path's category. `managed` and `preserved` hold relative,
/// `/`-separated paths.
pub(crate) struct Categories {
    pub(crate) managed: BTreeSet<String>,
    pub(crate) preserved: BTreeSet<String>,
}

impl Categories {
    fn of(&self, path: &str) -> FileCategory {
        if self.managed.contains(path) {
            FileCategory::Managed
        } else if self.preserved.contains(path) {
            FileCategory::Preserved
        } else {
            FileCategory::Unknown
        }
    }
}

/// Captures the tree below `root`.
///
/// `ignore` names entries the caller created itself (an active journal or
/// its temporary files); nothing else is ever skipped.
pub(crate) fn capture(
    root: &Dir,
    root_display: &str,
    categories: &Categories,
    ignore: &BTreeSet<String>,
) -> Result<ProjectSnapshot, SnapshotError> {
    let root_stat = root
        .stat_self()
        .map_err(|error| SnapshotError::Unreadable {
            path: String::new(),
            error,
        })?;
    let mut walker = Walker {
        categories,
        ignore,
        entries: Vec::new(),
        total_bytes: 0,
        file_count: 0,
        directory_count: 0,
    };
    walker.walk(root, "", 0)?;
    walker.entries.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(ProjectSnapshot {
        root: root_display.to_owned(),
        root_identity: FileIdentity {
            device: root_stat.dev,
            inode: root_stat.ino,
        },
        captured_at_unix_millis: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        file_count: walker.file_count,
        directory_count: walker.directory_count,
        total_bytes: walker.total_bytes,
        entries: walker.entries,
    })
}

struct Walker<'a> {
    categories: &'a Categories,
    ignore: &'a BTreeSet<String>,
    entries: Vec<SnapshotEntry>,
    total_bytes: u64,
    file_count: usize,
    directory_count: usize,
}

impl Walker<'_> {
    fn walk(&mut self, dir: &Dir, prefix: &str, depth: usize) -> Result<(), SnapshotError> {
        if depth > MAX_SNAPSHOT_DEPTH {
            return Err(SnapshotError::TooDeep {
                path: prefix.to_owned(),
            });
        }
        let remaining = MAX_SNAPSHOT_ENTRIES.saturating_sub(self.entries.len());
        let names = dir.entry_names(remaining).map_err(|error| match error {
            FsError::TooLarge => SnapshotError::TooManyEntries,
            error => SnapshotError::Unreadable {
                path: prefix.to_owned(),
                error,
            },
        })?;

        for name in names {
            let text =
                String::from_utf8(name.clone()).map_err(|_| SnapshotError::UnsupportedName {
                    path: format!("{prefix}<non-utf8>"),
                })?;
            let path = if prefix.is_empty() {
                text.clone()
            } else {
                format!("{prefix}{text}")
            };
            if self.ignore.contains(&path) {
                continue;
            }
            if self.entries.len() >= MAX_SNAPSHOT_ENTRIES {
                return Err(SnapshotError::TooManyEntries);
            }
            let c_name = CString::new(name)
                .map_err(|_| SnapshotError::UnsupportedName { path: path.clone() })?;
            let stat = dir
                .stat_entry(&c_name)
                .map_err(|error| SnapshotError::Unreadable {
                    path: path.clone(),
                    error,
                })?
                .ok_or_else(|| SnapshotError::Unstable { path: path.clone() })?;

            match stat.kind {
                EntryKind::Directory => {
                    self.directory_count += 1;
                    self.entries
                        .push(self.entry(&path, EntryType::Directory, &stat, None));
                    let child =
                        dir.open_subdir(&c_name)
                            .map_err(|error| SnapshotError::Unreadable {
                                path: path.clone(),
                                error,
                            })?;
                    self.walk(&child, &format!("{path}/"), depth + 1)?;
                }
                EntryKind::RegularFile => {
                    let hash = self.hash_file(dir, &c_name, &path, &stat)?;
                    self.file_count += 1;
                    self.entries
                        .push(self.entry(&path, EntryType::File, &stat, Some(hash)));
                }
                EntryKind::Symlink => {
                    // Tracked, never read: `readlink` would update the
                    // link's own access time, and following it is refused
                    // everywhere else in this crate.
                    self.entries
                        .push(self.entry(&path, EntryType::Symlink, &stat, None));
                }
                EntryKind::Other => {
                    self.entries
                        .push(self.entry(&path, EntryType::Special, &stat, None));
                }
                EntryKind::Missing => return Err(SnapshotError::Unstable { path }),
            }
        }
        Ok(())
    }

    fn entry(
        &self,
        path: &str,
        entry_type: EntryType,
        stat: &FileStat,
        hash: Option<String>,
    ) -> SnapshotEntry {
        SnapshotEntry {
            path: path.to_owned(),
            entry_type,
            category: self.categories.of(path),
            identity: FileIdentity {
                device: stat.dev,
                inode: stat.ino,
            },
            mode: stat.mode,
            uid: stat.uid,
            gid: stat.gid,
            link_count: stat.nlink,
            size: stat.size,
            mtime_secs: stat.mtime_secs,
            mtime_nanos: stat.mtime_nanos,
            ctime_secs: stat.ctime_secs,
            ctime_nanos: stat.ctime_nanos,
            content_sha256: hash,
        }
    }

    /// Hashes a regular file, bounded, and re-checks its metadata afterwards
    /// so a file rewritten mid-read is reported rather than recorded.
    fn hash_file(
        &mut self,
        dir: &Dir,
        name: &CString,
        path: &str,
        expected: &FileStat,
    ) -> Result<String, SnapshotError> {
        if expected.size > MAX_SNAPSHOT_FILE_BYTES
            || self.total_bytes.saturating_add(expected.size) > MAX_SNAPSHOT_BYTES
        {
            return Err(SnapshotError::TooLarge {
                path: path.to_owned(),
            });
        }
        let (mut file, opened) =
            dir.open_regular(name)
                .map_err(|error| SnapshotError::Unreadable {
                    path: path.to_owned(),
                    error,
                })?;
        if !opened.same_file(expected) {
            return Err(SnapshotError::Unstable {
                path: path.to_owned(),
            });
        }

        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; HASH_CHUNK];
        let mut read_total = 0u64;
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| SnapshotError::Unreadable {
                    path: path.to_owned(),
                    error: FsError::Io,
                })?;
            if read == 0 {
                break;
            }
            read_total += read as u64;
            if read_total > MAX_SNAPSHOT_FILE_BYTES
                || self.total_bytes.saturating_add(read_total) > MAX_SNAPSHOT_BYTES
            {
                return Err(SnapshotError::TooLarge {
                    path: path.to_owned(),
                });
            }
            hasher.update(&buffer[..read]);
        }

        let after =
            crate::fs_linux::stat_open_file(&file).map_err(|error| SnapshotError::Unreadable {
                path: path.to_owned(),
                error,
            })?;
        if !after.same_file(expected) || read_total != expected.size {
            return Err(SnapshotError::Unstable {
                path: path.to_owned(),
            });
        }
        self.total_bytes += read_total;
        Ok(hex(&hasher.finalize()))
    }
}

/// Lowercase hex, so hashes compare and serialize as plain strings.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        out.push(char::from_digit(u32::from(byte & 0x0F), 16).unwrap_or('0'));
    }
    out
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, hash: &str) -> SnapshotEntry {
        SnapshotEntry {
            path: path.to_owned(),
            entry_type: EntryType::File,
            category: FileCategory::Unknown,
            identity: FileIdentity {
                device: 1,
                inode: 2,
            },
            mode: 0o100_644,
            uid: 1000,
            gid: 1000,
            link_count: 1,
            size: 3,
            mtime_secs: 10,
            mtime_nanos: 0,
            ctime_secs: 10,
            ctime_nanos: 0,
            content_sha256: Some(hash.to_owned()),
        }
    }

    fn snapshot(entries: Vec<SnapshotEntry>) -> ProjectSnapshot {
        ProjectSnapshot {
            root: "/p".to_owned(),
            root_identity: FileIdentity {
                device: 1,
                inode: 1,
            },
            captured_at_unix_millis: 0,
            file_count: entries.len(),
            directory_count: 0,
            total_bytes: 0,
            entries,
        }
    }

    #[test]
    fn hex_matches_known_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn diff_detects_every_change_kind() {
        let base = snapshot(vec![entry("a.json", "aa"), entry("b.json", "bb")]);
        assert!(base.diff(&base, &BTreeSet::new()).is_empty());

        let mut modified = base.clone();
        modified.entries[0].content_sha256 = Some("cc".to_owned());
        assert_eq!(
            base.diff(&modified, &BTreeSet::new()).changes,
            vec![EntryChange {
                path: "a.json".to_owned(),
                change: ChangeKind::ContentModified,
            }]
        );

        let mut replaced = base.clone();
        replaced.entries[0].identity.inode = 99;
        assert_eq!(
            base.diff(&replaced, &BTreeSet::new()).changes[0].change,
            ChangeKind::Replaced
        );

        let mut retyped = base.clone();
        retyped.entries[1].entry_type = EntryType::Directory;
        assert_eq!(
            base.diff(&retyped, &BTreeSet::new()).changes[0].change,
            ChangeKind::TypeChanged
        );

        for mutate in [
            |e: &mut SnapshotEntry| e.mode = 0o100_600,
            |e: &mut SnapshotEntry| e.uid = 0,
            |e: &mut SnapshotEntry| e.gid = 0,
            |e: &mut SnapshotEntry| e.link_count = 2,
            |e: &mut SnapshotEntry| e.mtime_nanos = 5,
            |e: &mut SnapshotEntry| e.ctime_secs = 11,
        ] {
            let mut changed = base.clone();
            mutate(&mut changed.entries[0]);
            assert_eq!(
                base.diff(&changed, &BTreeSet::new()).changes[0].change,
                ChangeKind::MetadataChanged
            );
        }

        let mut removed = base.clone();
        removed.entries.pop();
        assert_eq!(
            base.diff(&removed, &BTreeSet::new()).changes[0].change,
            ChangeKind::Removed
        );

        let mut added = base.clone();
        added.entries.push(entry("c.json", "cc"));
        assert_eq!(
            base.diff(&added, &BTreeSet::new()).changes[0].change,
            ChangeKind::Added
        );

        let mut rerooted = base.clone();
        rerooted.root_identity.inode = 7;
        assert_eq!(
            base.diff(&rerooted, &BTreeSet::new()).changes[0].change,
            ChangeKind::RootReplaced
        );
    }

    #[test]
    fn diff_ignores_only_named_entries() {
        let base = snapshot(vec![entry("a.json", "aa")]);
        let mut added = base.clone();
        added.entries.push(entry(".biforgeworks-save", "zz"));
        added.entries.sort_by(|a, b| a.path.cmp(&b.path));
        assert!(!base.diff(&added, &BTreeSet::new()).is_empty());
        let ignore = BTreeSet::from([".biforgeworks-save".to_owned()]);
        assert!(base.diff(&added, &ignore).is_empty());
    }

    #[test]
    fn entry_lookup_uses_sorted_paths() {
        let snapshot = snapshot(vec![entry("a.json", "aa"), entry("b.json", "bb")]);
        assert_eq!(
            snapshot.entry("b.json").unwrap().content_sha256.as_deref(),
            Some("bb")
        );
        assert!(snapshot.entry("c.json").is_none());
    }
}
