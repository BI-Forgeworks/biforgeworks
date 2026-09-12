//! Descriptor-relative, read-only filesystem access for discovery (Linux).
//!
//! Guarantees relied on by discovery:
//!
//! - Every lookup below the project root is relative to an already-open
//!   directory descriptor (`openat`/`fstatat`) and never follows symlinks
//!   (`O_NOFOLLOW`, `AT_SYMLINK_NOFOLLOW`), so a concurrently swapped path
//!   component cannot redirect discovery outside the project.
//! - Directories are opened with `O_PATH` and never listed, so no directory
//!   access time changes.
//! - Files are only opened for reading with `O_NOATIME`. If the kernel
//!   refuses `O_NOATIME` (the caller does not own the file), the read fails
//!   rather than falling back to an atime-updating open.
//! - Files are checked to be regular both before opening (`fstatat`) and on
//!   the opened descriptor (`fstat`); `O_NONBLOCK` keeps a FIFO swapped in
//!   between those checks from blocking the open.
//! - Reads are bounded.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const DIR_FLAGS: libc::c_int = libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC;
const FILE_FLAGS: libc::c_int = libc::O_RDONLY
    | libc::O_NOFOLLOW
    | libc::O_NOATIME
    | libc::O_NONBLOCK
    | libc::O_NOCTTY
    | libc::O_CLOEXEC;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Missing,
    Directory,
    RegularFile,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FsError {
    NotFound,
    Symlink,
    NotDirectory,
    NotRegularFile,
    TooLarge,
    AccessDenied,
    /// The file could not be opened with `O_NOATIME` (typically `EPERM`
    /// because the caller does not own it).
    NoAtimeUnavailable,
    InvalidName,
    /// The destination already exists where exclusive creation was required.
    AlreadyExists,
    /// A file changed underneath a read that had to be stable.
    Unstable,
    /// The file's metadata cannot be reproduced or preserved.
    Unsupported,
    Io,
}

/// `lstat`-derived facts used for identity, preservation, and conflict
/// checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileStat {
    pub(crate) kind: EntryKind,
    pub(crate) dev: u64,
    pub(crate) ino: u64,
    pub(crate) mode: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) nlink: u64,
    pub(crate) size: u64,
    pub(crate) mtime_secs: i64,
    pub(crate) mtime_nanos: i64,
    pub(crate) ctime_secs: i64,
    pub(crate) ctime_nanos: i64,
}

impl FileStat {
    // `libc::stat` field types vary by architecture (`nlink_t`, `time_t`,
    // and `dev_t` are not the same width everywhere), so these casts are
    // redundant on x86-64 and load-bearing elsewhere.
    #[allow(clippy::unnecessary_cast)]
    fn from_stat(stat: &libc::stat) -> Self {
        Self {
            kind: kind_from_mode(stat.st_mode),
            dev: stat.st_dev as u64,
            ino: stat.st_ino as u64,
            mode: stat.st_mode as u32,
            uid: stat.st_uid as u32,
            gid: stat.st_gid as u32,
            nlink: stat.st_nlink as u64,
            size: stat.st_size.max(0) as u64,
            mtime_secs: stat.st_mtime as i64,
            mtime_nanos: stat.st_mtime_nsec as i64,
            ctime_secs: stat.st_ctime as i64,
            ctime_nanos: stat.st_ctime_nsec as i64,
        }
    }

    /// Whether two observations describe the same unchanged file.
    pub(crate) fn same_file(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.dev == other.dev
            && self.ino == other.ino
            && self.size == other.size
            && self.mtime_secs == other.mtime_secs
            && self.mtime_nanos == other.mtime_nanos
    }
}

/// An open directory handle (`O_PATH`), usable only as a lookup base.
pub(crate) struct Dir {
    fd: OwnedFd,
}

fn last_errno() -> Option<i32> {
    io::Error::last_os_error().raw_os_error()
}

fn kind_from_mode(mode: libc::mode_t) -> EntryKind {
    match mode & libc::S_IFMT {
        libc::S_IFDIR => EntryKind::Directory,
        libc::S_IFREG => EntryKind::RegularFile,
        libc::S_IFLNK => EntryKind::Symlink,
        _ => EntryKind::Other,
    }
}

fn map_lookup_errno(errno: Option<i32>) -> FsError {
    match errno {
        Some(libc::ENOENT) => FsError::NotFound,
        Some(libc::ELOOP) => FsError::Symlink,
        Some(libc::ENOTDIR) => FsError::NotDirectory,
        Some(libc::EACCES) => FsError::AccessDenied,
        Some(libc::ENAMETOOLONG) => FsError::InvalidName,
        Some(libc::EEXIST) => FsError::AlreadyExists,
        _ => FsError::Io,
    }
}

/// Maps the errno of a failed `O_NOATIME` file open. `EPERM` is the
/// documented `O_NOATIME` ownership refusal; it is never retried without the
/// flag.
fn map_file_open_errno(errno: Option<i32>) -> FsError {
    match errno {
        Some(libc::EPERM) => FsError::NoAtimeUnavailable,
        // Sockets cannot be opened; devices/FIFOs are rejected earlier.
        Some(libc::ENXIO) | Some(libc::ENODEV) => FsError::NotRegularFile,
        other => map_lookup_errno(other),
    }
}

/// Converts a single path segment to a C string, rejecting anything that
/// could address more than one directory level.
pub(crate) fn segment_name(segment: &[u8]) -> Result<CString, FsError> {
    if segment.is_empty() || segment == b"." || segment == b".." || segment.contains(&b'/') {
        return Err(FsError::InvalidName);
    }
    CString::new(segment).map_err(|_| FsError::InvalidName)
}

fn fstat_fd(fd: &OwnedFd) -> Result<libc::stat, FsError> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `fd` is an open descriptor owned by the caller and `stat`
    // points to writable storage of the correct type and size.
    let rc = unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) };
    if rc != 0 {
        return Err(map_lookup_errno(last_errno()));
    }
    // SAFETY: `fstat` returned success, so it fully initialized `stat`.
    Ok(unsafe { stat.assume_init() })
}

/// Takes ownership of a descriptor returned by `open`/`openat`.
fn owned(fd: libc::c_int) -> OwnedFd {
    // SAFETY: callers pass only non-negative descriptors freshly returned by
    // a successful `open`/`openat`, which nothing else owns.
    unsafe { OwnedFd::from_raw_fd(fd) }
}

impl Dir {
    /// Opens the directory containing the user-selected `.pbip`.
    ///
    /// Symlinks in this path are followed: it is the user's own selection
    /// (for example a symlinked home directory), not untrusted project
    /// metadata. Everything below it is opened relative to this handle.
    pub(crate) fn open_selected_root(path: &Path) -> Result<Dir, FsError> {
        let path = if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            path
        };
        let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| FsError::InvalidName)?;
        // SAFETY: `c_path` is a valid NUL-terminated string that outlives the
        // call; `open` does not retain the pointer.
        let fd = unsafe { libc::open(c_path.as_ptr(), DIR_FLAGS) };
        if fd < 0 {
            return Err(map_lookup_errno(last_errno()));
        }
        let dir = Dir { fd: owned(fd) };
        dir.verify_directory()?;
        Ok(dir)
    }

    /// Duplicates this handle (the same open directory, not a new lookup).
    pub(crate) fn try_clone(&self) -> Result<Dir, FsError> {
        Ok(Dir {
            fd: self.fd.try_clone().map_err(|_| FsError::Io)?,
        })
    }

    fn verify_directory(&self) -> Result<(), FsError> {
        match kind_from_mode(fstat_fd(&self.fd)?.st_mode) {
            EntryKind::Directory => Ok(()),
            _ => Err(FsError::NotDirectory),
        }
    }

    /// Classifies `name` in this directory without following symlinks and
    /// without opening it.
    pub(crate) fn entry_kind(&self, name: &CStr) -> Result<EntryKind, FsError> {
        let mut stat = MaybeUninit::<libc::stat>::uninit();
        // SAFETY: `self.fd` is an open directory descriptor, `name` is a
        // valid NUL-terminated string, and `stat` points to writable storage
        // of the correct type and size.
        let rc = unsafe {
            libc::fstatat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            return match map_lookup_errno(last_errno()) {
                FsError::NotFound => Ok(EntryKind::Missing),
                other => Err(other),
            };
        }
        // SAFETY: `fstatat` returned success, so it fully initialized `stat`.
        Ok(kind_from_mode(unsafe { stat.assume_init() }.st_mode))
    }

    /// Opens the real (non-symlink) subdirectory `name`.
    pub(crate) fn open_subdir(&self, name: &CStr) -> Result<Dir, FsError> {
        match self.entry_kind(name)? {
            EntryKind::Directory => {}
            EntryKind::Missing => return Err(FsError::NotFound),
            EntryKind::Symlink => return Err(FsError::Symlink),
            EntryKind::RegularFile | EntryKind::Other => return Err(FsError::NotDirectory),
        }
        // SAFETY: `self.fd` is an open directory descriptor and `name` is a
        // valid NUL-terminated string; `openat` does not retain the pointer.
        let fd = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                DIR_FLAGS | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(map_lookup_errno(last_errno()));
        }
        let dir = Dir { fd: owned(fd) };
        dir.verify_directory()?;
        Ok(dir)
    }

    /// Reads the regular file `name` in this directory, up to `limit` bytes,
    /// without following symlinks and without updating its access time.
    pub(crate) fn read_file(&self, name: &CStr, limit: u64) -> Result<Vec<u8>, FsError> {
        match self.entry_kind(name)? {
            EntryKind::RegularFile => {}
            EntryKind::Missing => return Err(FsError::NotFound),
            EntryKind::Symlink => return Err(FsError::Symlink),
            EntryKind::Directory | EntryKind::Other => return Err(FsError::NotRegularFile),
        }
        // SAFETY: `self.fd` is an open directory descriptor and `name` is a
        // valid NUL-terminated string; `openat` does not retain the pointer.
        let fd = unsafe { libc::openat(self.fd.as_raw_fd(), name.as_ptr(), FILE_FLAGS) };
        if fd < 0 {
            return Err(map_file_open_errno(last_errno()));
        }
        let fd = owned(fd);
        let stat = fstat_fd(&fd)?;
        if kind_from_mode(stat.st_mode) != EntryKind::RegularFile {
            return Err(FsError::NotRegularFile);
        }
        if u64::try_from(stat.st_size).map_or(true, |size| size > limit) {
            return Err(FsError::TooLarge);
        }

        let mut bytes = Vec::new();
        File::from(fd)
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| FsError::Io)?;
        if bytes.len() as u64 > limit {
            // The file grew after `fstat`.
            return Err(FsError::TooLarge);
        }
        Ok(bytes)
    }
}

/// Flags for creating a replacement file: exclusive, never through a
/// symlink, never truncating an existing file.
const CREATE_FILE_FLAGS: libc::c_int =
    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;

/// Directory handle reopened for reading, used for `getdents64` (with
/// `O_NOATIME`, so enumeration does not update the directory's access time)
/// and for `fsync`, which `O_PATH` descriptors cannot do.
struct ReadableDir {
    fd: OwnedFd,
}

/// Snapshot and transaction support: enumeration, streaming reads, and the
/// narrow set of write operations used to replace managed files.
impl Dir {
    fn open_readable(&self) -> Result<ReadableDir, FsError> {
        // SAFETY: `self.fd` is an open directory descriptor and `"."` is a
        // valid NUL-terminated name that cannot be a symlink.
        let fd = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOATIME | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(map_file_open_errno(last_errno()));
        }
        Ok(ReadableDir { fd: owned(fd) })
    }

    /// `lstat`s `name`; `Ok(None)` means the entry does not exist.
    pub(crate) fn stat_entry(&self, name: &CStr) -> Result<Option<FileStat>, FsError> {
        let mut stat = MaybeUninit::<libc::stat>::uninit();
        // SAFETY: open directory descriptor, valid name, writable `stat`.
        let rc = unsafe {
            libc::fstatat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            return match map_lookup_errno(last_errno()) {
                FsError::NotFound => Ok(None),
                other => Err(other),
            };
        }
        // SAFETY: `fstatat` succeeded and initialized `stat`.
        Ok(Some(FileStat::from_stat(&unsafe { stat.assume_init() })))
    }

    /// `fstat`s this directory itself (its identity for locking and for
    /// detecting a replaced project root).
    pub(crate) fn stat_self(&self) -> Result<FileStat, FsError> {
        Ok(FileStat::from_stat(&fstat_fd(&self.fd)?))
    }

    /// Lists entry names (excluding `.` and `..`), sorted, without updating
    /// the directory's access time.
    ///
    /// Enumeration stops with [`FsError::TooLarge`] as soon as more than
    /// `max_entries` names have been seen, so a hostile directory cannot
    /// force an unbounded allocation before the caller's own limits apply.
    pub(crate) fn entry_names(&self, max_entries: usize) -> Result<Vec<Vec<u8>>, FsError> {
        const HEADER: usize = 19; // d_ino, d_off, d_reclen, d_type
        let dir = self.open_readable()?;
        let mut buffer = vec![0u8; 32 * 1024];
        let mut names = Vec::new();
        loop {
            // SAFETY: `dir.fd` is an open directory descriptor and the
            // kernel writes at most `buffer.len()` bytes into `buffer`.
            let read = unsafe {
                libc::syscall(
                    libc::SYS_getdents64,
                    dir.fd.as_raw_fd(),
                    buffer.as_mut_ptr().cast::<libc::c_void>(),
                    buffer.len(),
                )
            };
            if read < 0 {
                return Err(map_lookup_errno(last_errno()));
            }
            let read = usize::try_from(read).map_err(|_| FsError::Io)?;
            if read == 0 {
                break;
            }
            let mut offset = 0usize;
            while offset < read {
                let record = buffer.get(offset..read).ok_or(FsError::Io)?;
                let length = record.get(16..18).ok_or(FsError::Io)?;
                let length = usize::from(u16::from_ne_bytes([length[0], length[1]]));
                if length < HEADER || offset + length > read {
                    return Err(FsError::Io);
                }
                let name = record.get(HEADER..length).ok_or(FsError::Io)?;
                let end = name.iter().position(|byte| *byte == 0).ok_or(FsError::Io)?;
                let name = &name[..end];
                if name != b"." && name != b".." {
                    if names.len() >= max_entries {
                        return Err(FsError::TooLarge);
                    }
                    names.push(name.to_vec());
                }
                offset += length;
            }
        }
        names.sort();
        Ok(names)
    }

    /// Flushes this directory's own entries (rename durability).
    pub(crate) fn sync(&self) -> Result<(), FsError> {
        let dir = self.open_readable()?;
        // SAFETY: `dir.fd` is an open directory descriptor.
        if unsafe { libc::fsync(dir.fd.as_raw_fd()) } != 0 {
            return Err(map_lookup_errno(last_errno()));
        }
        Ok(())
    }

    /// Opens a regular file for reading without following symlinks or
    /// updating its access time, returning its state at open time.
    pub(crate) fn open_regular(&self, name: &CStr) -> Result<(File, FileStat), FsError> {
        match self.entry_kind(name)? {
            EntryKind::RegularFile => {}
            EntryKind::Missing => return Err(FsError::NotFound),
            EntryKind::Symlink => return Err(FsError::Symlink),
            EntryKind::Directory | EntryKind::Other => return Err(FsError::NotRegularFile),
        }
        // SAFETY: open directory descriptor and valid NUL-terminated name.
        let fd = unsafe { libc::openat(self.fd.as_raw_fd(), name.as_ptr(), FILE_FLAGS) };
        if fd < 0 {
            return Err(map_file_open_errno(last_errno()));
        }
        let fd = owned(fd);
        let stat = FileStat::from_stat(&fstat_fd(&fd)?);
        if stat.kind != EntryKind::RegularFile {
            return Err(FsError::NotRegularFile);
        }
        Ok((File::from(fd), stat))
    }

    /// Creates `name` exclusively with `mode`; fails if anything is already
    /// there.
    pub(crate) fn create_new_file(&self, name: &CStr, mode: u32) -> Result<File, FsError> {
        // SAFETY: open directory descriptor and valid NUL-terminated name.
        let fd = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                CREATE_FILE_FLAGS,
                libc::c_uint::from(mode & 0o777),
            )
        };
        if fd < 0 {
            return Err(map_lookup_errno(last_errno()));
        }
        Ok(File::from(owned(fd)))
    }

    pub(crate) fn create_dir_exclusive(&self, name: &CStr, mode: u32) -> Result<(), FsError> {
        // SAFETY: open directory descriptor and valid NUL-terminated name.
        if unsafe { libc::mkdirat(self.fd.as_raw_fd(), name.as_ptr(), mode & 0o777) } != 0 {
            return Err(map_lookup_errno(last_errno()));
        }
        Ok(())
    }

    /// Atomically replaces `to` with `from` inside this directory.
    pub(crate) fn rename_entry(&self, from: &CStr, to: &CStr) -> Result<(), FsError> {
        // SAFETY: both names are valid and relative to this open directory.
        let rc = unsafe {
            libc::renameat(
                self.fd.as_raw_fd(),
                from.as_ptr(),
                self.fd.as_raw_fd(),
                to.as_ptr(),
            )
        };
        if rc != 0 {
            return Err(map_lookup_errno(last_errno()));
        }
        Ok(())
    }

    pub(crate) fn unlink_entry(&self, name: &CStr) -> Result<(), FsError> {
        // SAFETY: open directory descriptor and valid NUL-terminated name.
        if unsafe { libc::unlinkat(self.fd.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(map_lookup_errno(last_errno()));
        }
        Ok(())
    }

    pub(crate) fn remove_dir(&self, name: &CStr) -> Result<(), FsError> {
        // SAFETY: open directory descriptor and valid NUL-terminated name.
        let rc = unsafe { libc::unlinkat(self.fd.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
        if rc != 0 {
            return Err(map_lookup_errno(last_errno()));
        }
        Ok(())
    }
}

/// `fstat`s an already-open file (used to confirm a read was stable).
pub(crate) fn stat_open_file(file: &File) -> Result<FileStat, FsError> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `file` is open and `stat` points to writable storage of the
    // correct type and size.
    let rc = unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) };
    if rc != 0 {
        return Err(map_lookup_errno(last_errno()));
    }
    // SAFETY: `fstat` succeeded and initialized `stat`.
    Ok(FileStat::from_stat(&unsafe { stat.assume_init() }))
}

/// Whether a file carries extended attributes (which include POSIX ACLs).
/// Replacement by rename cannot carry them across, so managed targets that
/// have any are refused.
pub(crate) fn has_extended_attributes(file: &File) -> Result<bool, FsError> {
    // SAFETY: `file` is open; a null buffer with zero size asks only for the
    // size of the attribute-name list.
    let size = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
    if size < 0 {
        return match last_errno() {
            // Filesystems without xattr support cannot hold any.
            Some(libc::ENOTSUP) => Ok(false),
            other => Err(map_lookup_errno(other)),
        };
    }
    Ok(size > 0)
}

/// Flushes file contents and metadata to stable storage.
pub(crate) fn sync_file(file: &File) -> Result<(), FsError> {
    // SAFETY: `file` is an open descriptor owned by the caller.
    if unsafe { libc::fsync(file.as_raw_fd()) } != 0 {
        return Err(map_lookup_errno(last_errno()));
    }
    Ok(())
}

/// Applies `gid` to an open file, so a replacement keeps the original
/// group. The owning user is never changed: targets whose owner this
/// process cannot reproduce are refused before any write.
pub(crate) fn set_group(file: &File, gid: u32) -> Result<(), FsError> {
    // SAFETY: `file` is an open descriptor owned by the caller; `-1` leaves
    // the owning user untouched.
    if unsafe { libc::fchown(file.as_raw_fd(), u32::MAX, gid) } != 0 {
        return Err(map_lookup_errno(last_errno()));
    }
    Ok(())
}

/// This process's effective user and group.
pub(crate) fn effective_ids() -> (u32, u32) {
    // SAFETY: neither call has preconditions or can fail.
    unsafe { (libc::geteuid(), libc::getegid()) }
}

/// Applies `mode` to an open file (replacements keep the original mode).
pub(crate) fn set_mode(file: &File, mode: u32) -> Result<(), FsError> {
    // SAFETY: `file` is an open descriptor owned by the caller.
    if unsafe { libc::fchmod(file.as_raw_fd(), mode & 0o7777) } != 0 {
        return Err(map_lookup_errno(last_errno()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_names_reject_multi_level_forms() {
        for bad in [&b""[..], b".", b"..", b"a/b", b"/", b"a\0b"] {
            assert_eq!(segment_name(bad), Err(FsError::InvalidName), "{bad:?}");
        }
        assert!(segment_name(b"Sales.Report").is_ok());
    }

    #[test]
    fn noatime_refusal_fails_closed() {
        assert_eq!(
            map_file_open_errno(Some(libc::EPERM)),
            FsError::NoAtimeUnavailable
        );
        assert_eq!(map_file_open_errno(Some(libc::ELOOP)), FsError::Symlink);
        assert_eq!(
            map_file_open_errno(Some(libc::ENXIO)),
            FsError::NotRegularFile
        );
    }

    #[test]
    fn file_flags_include_read_only_safety_flags() {
        for flag in [libc::O_NOATIME, libc::O_NOFOLLOW, libc::O_NONBLOCK] {
            assert_eq!(FILE_FLAGS & flag, flag);
        }
        assert_eq!(FILE_FLAGS & libc::O_ACCMODE, libc::O_RDONLY);
        for forbidden in [libc::O_WRONLY, libc::O_RDWR, libc::O_CREAT, libc::O_TRUNC] {
            assert_eq!(FILE_FLAGS & forbidden, 0);
            assert_eq!(DIR_FLAGS & forbidden, 0);
        }
    }
}
