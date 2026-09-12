//! Zero-write guarantee: discovery must not change any content, metadata,
//! timestamp (including atime), or directory listing inside the project.
//!
//! Each project is copied into a temporary directory, every file and
//! directory is given a deliberately stale atime (which any ordinary read
//! would advance, even under `relatime`), and full `lstat` metadata plus
//! contents are compared before and after discovery.

#![cfg(target_os = "linux")]

mod common;

use biforgeworks_powerbi::discover_project;
use common::*;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::Path;

/// Asserts that discovering `pbip` leaves everything under `tree` untouched.
fn assert_discovery_is_read_only(tree: &Path, pbip: &Path) {
    let paths = collect_paths(tree);
    let contents = read_contents(&paths);
    let links = link_targets(&paths);
    set_stale_times(&paths);
    let before = stat_all(&paths);

    let summary = discover_project(pbip);
    // Twice: repeated opens must not differ from the first.
    let again = discover_project(pbip);
    assert_eq!(summary, again);
    biforgeworks_powerbi::tmdl::inspect_project(pbip);
    biforgeworks_powerbi::tmdl::inspect_project(pbip);

    let after = stat_all(&paths);
    for ((path, b), (_, a)) in before.iter().zip(&after) {
        assert_eq!(b, a, "metadata changed for {}", path.display());
    }
    assert_eq!(before.len(), after.len());
    assert_eq!(read_contents(&paths), contents, "contents changed");
    assert_eq!(link_targets(&paths), links, "symlink targets changed");
    // Listing last, because reading directories may itself update atime.
    assert_eq!(
        collect_paths(tree),
        paths,
        "entries were created or removed"
    );
}

fn warn_if_atime_is_vacuous(scratch: &Path) {
    if !filesystem_updates_atime(scratch) {
        eprintln!(
            "note: {} does not update atime on read (noatime mount?); atime assertions are vacuous here",
            scratch.display()
        );
    }
}

#[test]
fn control_ordinary_read_would_be_detected() {
    // Demonstrates the harness is sensitive: an ordinary read of a stale
    // file is observable wherever the filesystem records atime.
    let tmp = TempDir::new("ro-control");
    if !filesystem_updates_atime(tmp.path()) {
        eprintln!("skipping: filesystem does not update atime");
        return;
    }
    let file = tmp.path().join("f.json");
    write(&file, "{}");
    let paths = vec![file.clone()];
    set_stale_times(&paths);
    let before = stat_all(&paths);
    let _ = fs::read(&file).unwrap();
    assert_ne!(before, stat_all(&paths));
}

#[test]
fn every_fixture_is_opened_read_only() {
    let tmp = TempDir::new("ro-fixtures");
    warn_if_atime_is_vacuous(tmp.path());
    for (fixture, pbip) in [
        ("valid-pbir-tmdl", "Sales.pbip"),
        ("valid-legacy", "Legacy.pbip"),
        ("missing-report", "MissingReport.pbip"),
        ("missing-model", "MissingModel.pbip"),
        ("malformed-pbip", "Malformed.pbip"),
        ("unknown-formats", "Unknown.pbip"),
        ("tmdl-star-schema", "Sales.pbip"),
        ("tmdl-minimal", "Sales.pbip"),
        ("tmdl-unknown-properties", "Sales.pbip"),
        ("tmdl-invalid-syntax", "Sales.pbip"),
        ("tmdl-broken-references", "Sales.pbip"),
    ] {
        let dir = copy_fixture(fixture, tmp.path());
        assert_discovery_is_read_only(&dir, &dir.join(pbip));
    }
}

#[test]
fn adversarial_projects_are_opened_read_only() {
    let tmp = TempDir::new("ro-adversarial");
    warn_if_atime_is_vacuous(tmp.path());

    // Symlinked folders and files, including links to outside targets whose
    // atime must also be untouched.
    let root = tmp.path().join("links");
    let pbip = build_pbir_tmdl(&root, "L");
    let outside = root.join("outside-target.json");
    write(&outside, "{}");
    fs::remove_file(root.join("L.SemanticModel/definition.pbism")).unwrap();
    symlink(&outside, root.join("L.SemanticModel/definition.pbism")).unwrap();
    fs::remove_file(root.join("L.Report/definition/version.json")).unwrap();
    symlink(
        "../../outside-target.json",
        root.join("L.Report/definition/version.json"),
    )
    .unwrap();
    assert_discovery_is_read_only(&root, &pbip);

    // FIFO metadata, oversized metadata, invalid UTF-8.
    let root = tmp.path().join("special");
    let pbip = build_pbir_tmdl(&root, "S");
    let fifo = root.join("S.Report/definition.pbir");
    fs::remove_file(&fifo).unwrap();
    let c_fifo = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c_fifo` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c_fifo.as_ptr(), 0o600) }, 0);
    assert_discovery_is_read_only(&root, &pbip);

    let root = tmp.path().join("large");
    let pbip = build_pbir_tmdl(&root, "B");
    write(
        &root.join("B.SemanticModel/definition.pbism"),
        vec![b' '; 2 * 1024 * 1024],
    );
    write(&root.join("B.Report/definition/version.json"), b"\xFF\xFE");
    assert_discovery_is_read_only(&root, &pbip);

    // Escaping and remote references.
    let root = tmp.path().join("escape");
    let pbip = build_pbir_tmdl(&root, "E");
    write(
        &root.join("E.Report/definition.pbir"),
        pbir_json("4.0", "../../elsewhere"),
    );
    assert_discovery_is_read_only(&root, &pbip);
}

#[test]
fn committed_fixtures_are_untouched() {
    // Metadata-only comparison of the committed tree (no stale-time setup,
    // since committed files must not be modified by tests either).
    let dir = fixtures_dir();
    let paths = collect_paths(&dir);
    let before = stat_all(&paths);
    for (fixture, pbip) in [
        ("valid-pbir-tmdl", "Sales.pbip"),
        ("valid-legacy", "Legacy.pbip"),
        ("unknown-formats", "Unknown.pbip"),
    ] {
        discover_project(&dir.join(fixture).join(pbip));
    }
    // Only files discovery could touch are compared; `collect_paths` itself
    // lists directories, which other tests may do concurrently.
    let files_before: Vec<_> = before.iter().filter(|(p, _)| p.is_file()).collect();
    let after = stat_all(&paths);
    let files_after: Vec<_> = after.iter().filter(|(p, _)| p.is_file()).collect();
    assert_eq!(files_before, files_after);
}
