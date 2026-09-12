//! Whole-tree comparison for WP02 preservation tests.
//!
//! Captures every entry below a project root — including the root
//! directory itself — with type, full `lstat` metadata (identity, mode,
//! ownership, link count, size, and every one of atime/mtime/ctime), and
//! (for regular files) exact bytes.
//!
//! `compare` treats every difference as a violation except three explicit,
//! narrow allowances the caller must name:
//! - `managed`: paths that may change content and metadata freely (the one
//!   file a transaction is allowed to rewrite).
//! - `bookkeeping_dirs`: directories whose own `size`/`mtime`/`ctime`/
//!   `nlink` may drift because entries were added or removed inside them
//!   (ordinary bookkeeping) — pass `""` to allow this for the project root
//!   itself. Nothing else about a bookkeeping directory is exempt: its
//!   identity, type, mode, ownership, and **atime** must still match
//!   exactly, and every unlisted directory (an unrelated opaque
//!   subdirectory elsewhere in the tree) gets no exemption at all.
//! - `metadata_exempt`: file paths whose *metadata* (identity, mode,
//!   ownership, timestamps) is allowed to differ — because an atomic
//!   replace-then-rename necessarily mints a new inode even when restoring
//!   a file to its original bytes — while their *content* is still
//!   compared and must match exactly.
//!
//! atime is never exempted anywhere, including on bookkeeping directories:
//! adding or removing a directory entry updates that directory's mtime/
//! ctime, never its atime, so any atime drift always indicates something
//! (this helper's own capture included) read what it should not have.
//!
//! Directory enumeration and regular-file reads use `O_NOATIME`/`O_NOFOLLOW`
//! so capturing a snapshot never perturbs the very timestamps it records.
//! `lstat` itself never advances atime, so it is taken with plain
//! `fs::symlink_metadata`; symlink targets are read only via [`link_targets`],
//! called after every snapshot's metadata has already been captured, since
//! `readlink` itself updates the link's own access time — the same ordering
//! `tests/common/mod.rs` already uses for read-only discovery.
//!
//! Test-only: not part of the production crate, and not wired into any
//! production API.

#![allow(dead_code)]
#![cfg(target_os = "linux")]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CString, OsStr};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// Largest regular file this helper will read into memory. The committed
/// WP02 fixtures are all far smaller; a file over this bound fails the
/// capture loudly rather than being silently truncated or skipped.
pub const MAX_COMPARISON_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
    Symlink,
    Special,
}

fn kind_of(meta: &fs::Metadata) -> Kind {
    let file_type = meta.file_type();
    if file_type.is_dir() {
        Kind::Directory
    } else if file_type.is_symlink() {
        Kind::Symlink
    } else if file_type.is_file() {
        Kind::File
    } else {
        Kind::Special
    }
}

/// `lstat`-derived metadata compared between snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryMeta {
    pub kind: Kind,
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub atime: (i64, i64),
    pub mtime: (i64, i64),
    pub ctime: (i64, i64),
}

impl EntryMeta {
    fn capture(meta: &fs::Metadata) -> Self {
        Self {
            kind: kind_of(meta),
            dev: meta.dev(),
            ino: meta.ino(),
            mode: meta.mode(),
            nlink: meta.nlink(),
            uid: meta.uid(),
            gid: meta.gid(),
            size: meta.size(),
            atime: (meta.atime(), meta.atime_nsec()),
            mtime: (meta.mtime(), meta.mtime_nsec()),
            ctime: (meta.ctime(), meta.ctime_nsec()),
        }
    }
}

/// True when `before`/`after` differ in a way `compare` should report,
/// given whether `path` is allowed ordinary directory bookkeeping.
/// Identity, mode, ownership, and atime are never exempt; size/mtime/
/// ctime/nlink are exempt only when `bookkeeping_allowed`.
fn metadata_violation(before: &EntryMeta, after: &EntryMeta, bookkeeping_allowed: bool) -> bool {
    if before.dev != after.dev
        || before.ino != after.ino
        || before.mode != after.mode
        || before.uid != after.uid
        || before.gid != after.gid
        || before.atime != after.atime
    {
        return true;
    }
    if bookkeeping_allowed {
        return false;
    }
    before.nlink != after.nlink
        || before.size != after.size
        || before.mtime != after.mtime
        || before.ctime != after.ctime
}

/// A full-fidelity capture of a project root and every entry below it.
pub struct TreeSnapshot {
    root: PathBuf,
    /// The root directory's own metadata (not one of `paths`/`meta` below).
    pub root_meta: EntryMeta,
    /// Root-relative, `/`-separated paths, sorted.
    pub paths: Vec<String>,
    pub meta: BTreeMap<String, EntryMeta>,
    /// Regular-file contents, keyed the same way; absent for anything that
    /// is not a regular file.
    pub contents: BTreeMap<String, Vec<u8>>,
}

/// Opens a directory with `O_NOATIME`/`O_NOFOLLOW`/`O_DIRECTORY` so listing
/// it does not update its own access time.
fn open_dir_noatime(path: &Path) -> OwnedFd {
    let c_path = CString::new(path.as_os_str().as_bytes()).expect("path has no NUL bytes");
    // SAFETY: `c_path` is a valid NUL-terminated string; `open` does not
    // retain the pointer beyond the call.
    let fd = unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_RDONLY
                | libc::O_DIRECTORY
                | libc::O_NOFOLLOW
                | libc::O_NOATIME
                | libc::O_CLOEXEC,
        )
    };
    assert!(
        fd >= 0,
        "open directory {}: {}",
        path.display(),
        std::io::Error::last_os_error()
    );
    // SAFETY: `fd` is a valid descriptor freshly returned by a successful
    // `open`, owned by nothing else.
    unsafe { OwnedFd::from_raw_fd(fd) }
}

/// Names of `path`'s direct children (no `.`/`..`), sorted, gathered
/// through `getdents64` on an `O_NOATIME` handle so enumeration does not
/// update the directory's own access time.
fn list_dir_noatime(path: &Path) -> Vec<Vec<u8>> {
    const HEADER: usize = 19; // d_ino, d_off, d_reclen, d_type
    let dir = open_dir_noatime(path);
    let mut buffer = vec![0u8; 32 * 1024];
    let mut names = Vec::new();
    loop {
        // SAFETY: `dir` is an open directory descriptor and the kernel
        // writes at most `buffer.len()` bytes into `buffer`.
        let read = unsafe {
            libc::syscall(
                libc::SYS_getdents64,
                dir.as_raw_fd(),
                buffer.as_mut_ptr().cast::<libc::c_void>(),
                buffer.len(),
            )
        };
        assert!(
            read >= 0,
            "getdents64 on {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
        let read = read as usize;
        if read == 0 {
            break;
        }
        let mut offset = 0usize;
        while offset < read {
            let record = &buffer[offset..read];
            let length = u16::from_ne_bytes([
                *record.get(16).expect("getdents64 record truncated"),
                *record.get(17).expect("getdents64 record truncated"),
            ]) as usize;
            assert!(
                length >= HEADER && offset + length <= read,
                "malformed getdents64 record"
            );
            let name = &record[HEADER..length];
            let end = name
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(name.len());
            let name = &name[..end];
            if name != b"." && name != b".." {
                names.push(name.to_vec());
            }
            offset += length;
        }
    }
    names.sort();
    names
}

/// Reads a regular file's bytes without following symlinks or updating its
/// access time. The read itself is bounded via `Read::take`, so a file
/// that grows after being listed cannot force an unbounded read.
fn read_noatime(path: &Path) -> Vec<u8> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOATIME | libc::O_NOFOLLOW)
        .open(path)
        .unwrap_or_else(|error| panic!("open {} with O_NOATIME: {error}", path.display()));
    let mut bytes = Vec::new();
    file.take(MAX_COMPARISON_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    assert!(
        bytes.len() as u64 <= MAX_COMPARISON_FILE_BYTES,
        "{} exceeds the {} byte comparison bound",
        path.display(),
        MAX_COMPARISON_FILE_BYTES
    );
    bytes
}

/// Captures the full tree rooted at `root`, `root` itself included.
pub fn capture(root: &Path) -> TreeSnapshot {
    let root_lstat = fs::symlink_metadata(root)
        .unwrap_or_else(|error| panic!("lstat {}: {error}", root.display()));
    let root_meta = EntryMeta::capture(&root_lstat);

    let mut paths = Vec::new();
    let mut meta = BTreeMap::new();
    let mut contents = BTreeMap::new();
    walk(root, root, &mut paths, &mut meta, &mut contents);
    paths.sort();
    TreeSnapshot {
        root: root.to_path_buf(),
        root_meta,
        paths,
        meta,
        contents,
    }
}

fn walk(
    root: &Path,
    dir: &Path,
    paths: &mut Vec<String>,
    meta: &mut BTreeMap<String, EntryMeta>,
    contents: &mut BTreeMap<String, Vec<u8>>,
) {
    for name in list_dir_noatime(dir) {
        let child = dir.join(OsStr::from_bytes(&name));
        let relative = child
            .strip_prefix(root)
            .expect("child path is under root")
            .to_owned();
        let relative = String::from_utf8(relative.into_os_string().into_vec())
            .unwrap_or_else(|_| panic!("non-UTF-8 path under {}", root.display()));

        let lstat = fs::symlink_metadata(&child)
            .unwrap_or_else(|error| panic!("lstat {}: {error}", child.display()));
        let entry_meta = EntryMeta::capture(&lstat);

        paths.push(relative.clone());
        if entry_meta.kind == Kind::File {
            contents.insert(relative.clone(), read_noatime(&child));
        }
        let is_dir = entry_meta.kind == Kind::Directory;
        meta.insert(relative, entry_meta);

        if is_dir {
            walk(root, &child, paths, meta, contents);
        }
    }
}

/// Symlink targets recorded in `snapshot`. Call only after every snapshot's
/// metadata has already been captured (via [`capture`]): `readlink` itself
/// updates the link's own access time, so reading targets any earlier would
/// perturb the very timestamps a preservation test is checking.
pub fn link_targets(snapshot: &TreeSnapshot) -> BTreeMap<String, PathBuf> {
    snapshot
        .meta
        .iter()
        .filter(|(_, entry)| entry.kind == Kind::Symlink)
        .map(|(relative, _)| {
            let full = snapshot.root.join(relative);
            let target = fs::read_link(&full)
                .unwrap_or_else(|error| panic!("readlink {}: {error}", full.display()));
            (relative.clone(), target)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    Added(String),
    Removed(String),
    TypeChanged(String),
    ContentChanged(String),
    /// The empty path names the project root itself.
    MetadataChanged(String),
}

/// Compares two snapshots of the same tree. See the module documentation
/// for exactly what `managed`, `bookkeeping_dirs`, and `metadata_exempt`
/// allow; everything else must match exactly, atime included.
pub fn compare(
    before: &TreeSnapshot,
    after: &TreeSnapshot,
    managed: &[&str],
    bookkeeping_dirs: &[&str],
    metadata_exempt: &[&str],
) -> Vec<Violation> {
    let managed: BTreeSet<&str> = managed.iter().copied().collect();
    let bookkeeping: BTreeSet<&str> = bookkeeping_dirs.iter().copied().collect();
    let metadata_exempt: BTreeSet<&str> = metadata_exempt.iter().copied().collect();
    let mut violations = Vec::new();

    if metadata_violation(
        &before.root_meta,
        &after.root_meta,
        bookkeeping.contains(""),
    ) {
        violations.push(Violation::MetadataChanged(String::new()));
    }

    for path in &before.paths {
        if managed.contains(path.as_str()) {
            continue;
        }
        let before_meta = &before.meta[path];
        let Some(after_meta) = after.meta.get(path) else {
            violations.push(Violation::Removed(path.clone()));
            continue;
        };
        if before_meta.kind != after_meta.kind {
            violations.push(Violation::TypeChanged(path.clone()));
            continue;
        }
        if !metadata_exempt.contains(path.as_str()) {
            let allow_bookkeeping =
                before_meta.kind == Kind::Directory && bookkeeping.contains(path.as_str());
            if metadata_violation(before_meta, after_meta, allow_bookkeeping) {
                violations.push(Violation::MetadataChanged(path.clone()));
            }
        }
        if before_meta.kind == Kind::File && before.contents[path] != after.contents[path] {
            violations.push(Violation::ContentChanged(path.clone()));
        }
    }

    for path in &after.paths {
        if !managed.contains(path.as_str()) && !before.meta.contains_key(path) {
            violations.push(Violation::Added(path.clone()));
        }
    }

    violations
}

/// Asserts `compare` finds no violations, printing every one otherwise.
pub fn assert_preserved(
    before: &TreeSnapshot,
    after: &TreeSnapshot,
    managed: &[&str],
    bookkeeping_dirs: &[&str],
    metadata_exempt: &[&str],
) {
    let violations = compare(before, after, managed, bookkeeping_dirs, metadata_exempt);
    assert!(
        violations.is_empty(),
        "unexpected tree differences (managed={managed:?}, bookkeeping_dirs={bookkeeping_dirs:?}, \
         metadata_exempt={metadata_exempt:?}): {violations:#?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, UNIX_EPOCH};

    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "biforgeworks-preservation-support-{tag}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create scratch dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn stale_times() -> fs::FileTimes {
        fs::FileTimes::new()
            .set_accessed(UNIX_EPOCH + Duration::from_secs(978_307_200)) // 2001-01-01
            .set_modified(UNIX_EPOCH + Duration::from_secs(1_009_843_200)) // 2002-01-01
    }

    #[test]
    fn capture_itself_never_perturbs_atime() {
        let scratch = ScratchDir::new("capture-noatime");
        let file = scratch.path().join("f.json");
        fs::write(&file, b"{}").unwrap();
        fs::File::open(&file)
            .unwrap()
            .set_times(stale_times())
            .unwrap();

        let before = capture(scratch.path());
        let after = capture(scratch.path());
        assert!(compare(&before, &after, &[], &[], &[]).is_empty());
    }

    #[test]
    fn atime_drift_on_a_file_is_detected() {
        let scratch = ScratchDir::new("atime");
        let file = scratch.path().join("f.json");
        fs::write(&file, b"{}").unwrap();
        fs::File::open(&file)
            .unwrap()
            .set_times(stale_times())
            .unwrap();

        let before = capture(scratch.path());
        // An ordinary read through the standard library (no O_NOATIME)
        // advances atime on any filesystem that honours it at all.
        let _ = fs::read(&file).unwrap();
        let after = capture(scratch.path());

        if before.meta["f.json"].atime == after.meta["f.json"].atime {
            eprintln!("skipping: filesystem does not update atime (noatime mount?)");
            return;
        }
        let violations = compare(&before, &after, &[], &[], &[]);
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, Violation::MetadataChanged(p) if p == "f.json")),
            "atime-only drift should be reported: {violations:?}"
        );
    }

    #[test]
    fn directory_bookkeeping_drift_is_detected_unless_allow_listed() {
        let scratch = ScratchDir::new("dir-bookkeeping");
        let sub = scratch.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let before = capture(scratch.path());

        // Adding a file inside `sub` bumps `sub`'s own mtime/ctime: real
        // bookkeeping, but only exempt where the caller names it.
        fs::write(sub.join("new.txt"), b"x").unwrap();
        let after = capture(scratch.path());

        let unrestricted = compare(&before, &after, &[], &[], &[]);
        assert!(
            unrestricted
                .iter()
                .any(|v| matches!(v, Violation::Added(p) if p == "sub/new.txt")),
            "the new file itself must always be reported: {unrestricted:?}"
        );
        assert!(
            unrestricted
                .iter()
                .any(|v| matches!(v, Violation::MetadataChanged(p) if p == "sub")),
            "sub's own bookkeeping drift must be reported when not allow-listed: {unrestricted:?}"
        );

        let allowed = compare(&before, &after, &[], &["sub"], &[]);
        assert!(
            !allowed
                .iter()
                .any(|v| matches!(v, Violation::MetadataChanged(p) if p == "sub")),
            "sub's bookkeeping drift must be exempt once allow-listed: {allowed:?}"
        );
        assert!(
            allowed
                .iter()
                .any(|v| matches!(v, Violation::Added(p) if p == "sub/new.txt")),
            "an allow-listed directory does not exempt files added inside it: {allowed:?}"
        );
    }

    #[test]
    fn root_bookkeeping_drift_is_detected_unless_allow_listed() {
        let scratch = ScratchDir::new("root-bookkeeping");
        let before = capture(scratch.path());
        fs::write(scratch.path().join("new.txt"), b"x").unwrap();
        let after = capture(scratch.path());

        let unrestricted = compare(&before, &after, &[], &[], &[]);
        assert!(
            unrestricted
                .iter()
                .any(|v| matches!(v, Violation::MetadataChanged(p) if p.is_empty())),
            "root's own bookkeeping drift must be reported when not allow-listed: {unrestricted:?}"
        );

        let allowed = compare(&before, &after, &[], &[""], &[]);
        assert!(
            !allowed
                .iter()
                .any(|v| matches!(v, Violation::MetadataChanged(p) if p.is_empty())),
            "root's bookkeeping drift must be exempt once allow-listed: {allowed:?}"
        );
    }

    #[test]
    fn metadata_exempt_file_still_has_its_content_checked() {
        let scratch = ScratchDir::new("metadata-exempt");
        let file = scratch.path().join("f.txt");
        fs::write(&file, b"original").unwrap();
        let before = capture(scratch.path());

        // Simulate an atomic replace-then-rename that restores the exact
        // original bytes through a brand-new inode.
        let temp = scratch.path().join("f.txt.tmp");
        fs::write(&temp, b"original").unwrap();
        fs::rename(&temp, &file).unwrap();
        let after = capture(scratch.path());

        // The temp-file-then-rename dance also bumps the scratch root's own
        // bookkeeping timestamps, which is not what this test is about.
        let violations = compare(&before, &after, &[], &[""], &["f.txt"]);
        assert!(
            violations.is_empty(),
            "identical content through a new inode must be clean once metadata-exempt: {violations:?}"
        );

        fs::write(&file, b"different").unwrap();
        let after_changed = capture(scratch.path());
        let violations = compare(&before, &after_changed, &[], &[""], &["f.txt"]);
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, Violation::ContentChanged(p) if p == "f.txt")),
            "metadata-exempt must never exempt content: {violations:?}"
        );
    }
}
