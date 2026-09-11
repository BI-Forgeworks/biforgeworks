//! Shared helpers for integration tests. Every mutable project is built in a
//! fresh temporary directory; committed fixtures are only ever copied.

#![allow(dead_code)]

use biforgeworks_powerbi::{DiagnosticCode, PowerBiProjectSummary, Severity};
use std::collections::BTreeMap;
use std::fs::{self, File, FileTimes, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const PBIP_SCHEMA: &str =
    "https://developer.microsoft.com/json-schemas/fabric/pbip/pbipProperties/1.0.0/schema.json";
pub const PBIR_SCHEMA_V2: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json";
pub const PBISM_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/semanticModel/definitionProperties/1.0.0/schema.json";
pub const VERSION_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/versionMetadata/1.0.0/schema.json";

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/powerbi")
}

/// A uniquely named temporary directory, removed (after restoring owner
/// permissions) on drop.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "biforgeworks-powerbi-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create temp dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        restore_permissions(&self.path);
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn restore_permissions(path: &Path) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                restore_permissions(&entry.path());
            }
        }
    } else if meta.is_file() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

/// Recursively copies a directory tree (no symlinks expected). Sources are
/// read with `O_NOATIME` so copying leaves committed fixtures' atimes alone.
pub fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create copy target");
    for entry in fs::read_dir(from).expect("read fixture dir") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        let kind = entry.file_type().expect("fixture file type");
        if kind.is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            let bytes = read_contents(&[entry.path()])
                .into_values()
                .next()
                .expect("fixture file readable");
            fs::write(&target, bytes).expect("copy fixture file");
        }
    }
}

/// Copies a named fixture into `dest/<name>` and returns that directory.
pub fn copy_fixture(name: &str, dest: &Path) -> PathBuf {
    let target = dest.join(name);
    copy_tree(&fixtures_dir().join(name), &target);
    target
}

pub fn write(path: &Path, contents: impl AsRef<[u8]>) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, contents).expect("write test file");
}

pub fn pbip_json(report_path: &str) -> String {
    serde_json::json!({
        "$schema": PBIP_SCHEMA,
        "version": "1.0",
        "artifacts": [{ "report": { "path": report_path } }],
    })
    .to_string()
}

pub fn pbir_json(version: &str, model_path: &str) -> String {
    serde_json::json!({
        "$schema": PBIR_SCHEMA_V2,
        "version": version,
        "datasetReference": { "byPath": { "path": model_path } },
    })
    .to_string()
}

pub fn pbism_json(version: &str) -> String {
    serde_json::json!({ "$schema": PBISM_SCHEMA, "version": version, "settings": {} }).to_string()
}

pub fn version_json(version: &str) -> String {
    serde_json::json!({ "$schema": VERSION_SCHEMA, "version": version }).to_string()
}

/// Builds `<root>/<name>.pbip`, a PBIR report folder, and a TMDL model
/// folder with the conventional names. Returns the `.pbip` path.
pub fn build_pbir_tmdl(root: &Path, name: &str) -> PathBuf {
    let report = root.join(format!("{name}.Report"));
    let model = root.join(format!("{name}.SemanticModel"));
    let pbip = root.join(format!("{name}.pbip"));
    write(&pbip, pbip_json(&format!("{name}.Report")));
    write(
        &report.join("definition.pbir"),
        pbir_json("4.0", &format!("../{name}.SemanticModel")),
    );
    write(
        &report.join("definition/version.json"),
        version_json("2.0.0"),
    );
    write(&report.join("definition/report.json"), "{}");
    write(&model.join("definition.pbism"), pbism_json("4.0"));
    write(&model.join("definition/model.tmdl"), "model Model\n");
    pbip
}

pub fn has(summary: &PowerBiProjectSummary, code: DiagnosticCode) -> bool {
    summary.diagnostics.iter().any(|d| d.code == code)
}

pub fn assert_has(summary: &PowerBiProjectSummary, code: DiagnosticCode) {
    assert!(
        has(summary, code),
        "expected {code:?} in diagnostics: {:#?}",
        summary.diagnostics
    );
}

pub fn has_error(summary: &PowerBiProjectSummary) -> bool {
    summary
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
}

/// Serializes the full summary; used to assert nothing sensitive leaks.
pub fn serialized(summary: &PowerBiProjectSummary) -> String {
    serde_json::to_string(summary).expect("summary serializes")
}

/// Every path in a tree (root included), without following symlinks.
pub fn collect_paths(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let mut index = 0;
    while index < out.len() {
        let path = out[index].clone();
        index += 1;
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            let mut children: Vec<PathBuf> = fs::read_dir(&path)
                .expect("read dir")
                .map(|e| e.expect("dir entry").path())
                .collect();
            children.sort();
            out.extend(children);
        }
    }
    out
}

/// Full `lstat` metadata relevant to "zero writes": identity, type, mode,
/// ownership, size, link count, and all three timestamps at nanosecond
/// resolution. Symlink targets are compared separately ([`link_targets`])
/// because `readlink` itself updates a symlink's atime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    dev: u64,
    ino: u64,
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    size: u64,
    atime: (i64, i64),
    mtime: (i64, i64),
    ctime: (i64, i64),
}

pub fn stat_all(paths: &[PathBuf]) -> Vec<(PathBuf, Meta)> {
    paths
        .iter()
        .map(|path| {
            let m = fs::symlink_metadata(path).expect("lstat");
            (
                path.clone(),
                Meta {
                    dev: m.dev(),
                    ino: m.ino(),
                    mode: m.mode(),
                    nlink: m.nlink(),
                    uid: m.uid(),
                    gid: m.gid(),
                    size: m.size(),
                    atime: (m.atime(), m.atime_nsec()),
                    mtime: (m.mtime(), m.mtime_nsec()),
                    ctime: (m.ctime(), m.ctime_nsec()),
                },
            )
        })
        .collect()
}

/// Symlink targets in a tree. Call only after metadata snapshots.
pub fn link_targets(paths: &[PathBuf]) -> BTreeMap<PathBuf, PathBuf> {
    paths
        .iter()
        .filter(|p| fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
        .map(|p| (p.clone(), fs::read_link(p).expect("readlink")))
        .collect()
}

/// Reads every regular file's bytes using `O_NOATIME` so that capturing
/// contents does not itself disturb access times.
pub fn read_contents(paths: &[PathBuf]) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    for path in paths {
        let meta = fs::symlink_metadata(path).expect("lstat");
        if !meta.file_type().is_file() || meta.mode() & 0o400 == 0 {
            continue;
        }
        let mut bytes = Vec::new();
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOATIME | libc::O_NOFOLLOW)
            .open(path)
            .expect("open with O_NOATIME")
            .read_to_end(&mut bytes)
            .expect("read contents");
        out.insert(path.clone(), bytes);
    }
    out
}

/// A deliberately stale access time: any ordinary read (even under
/// `relatime`) would advance it.
pub fn stale_times() -> FileTimes {
    FileTimes::new()
        .set_accessed(UNIX_EPOCH + Duration::from_secs(978_307_200)) // 2001-01-01
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_009_843_200)) // 2002-01-01
}

/// Sets stale atime/mtime on every regular file and directory (symlinks and
/// special files are left alone).
pub fn set_stale_times(paths: &[PathBuf]) {
    for path in paths {
        let meta = fs::symlink_metadata(path).expect("lstat");
        if !(meta.is_file() || meta.is_dir()) || meta.mode() & 0o400 == 0 {
            continue;
        }
        File::open(path)
            .expect("open for set_times")
            .set_times(stale_times())
            .expect("set stale times");
    }
}

/// Reports whether this filesystem advances a stale atime on an ordinary
/// read; if not (for example a `noatime` mount), atime assertions are
/// vacuous and the caller should say so.
pub fn filesystem_updates_atime(scratch: &Path) -> bool {
    let probe = scratch.join("atime-probe");
    fs::write(&probe, b"probe").expect("write probe");
    File::open(&probe)
        .expect("open probe")
        .set_times(stale_times())
        .expect("stale probe");
    let before = fs::metadata(&probe).expect("stat probe").atime();
    let _ = fs::read(&probe).expect("read probe");
    let after = fs::metadata(&probe).expect("stat probe").atime();
    fs::remove_file(&probe).expect("remove probe");
    before != after
}

pub fn is_root() -> bool {
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}
