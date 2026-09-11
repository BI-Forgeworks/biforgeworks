//! Untrusted-input handling: escapes, absolute references, symlinks,
//! non-regular files, oversized and non-UTF-8 metadata, and no panics.

#![cfg(target_os = "linux")]

mod common;

use biforgeworks_powerbi::{
    discover_project, ComponentFormat, DiagnosticCode as Code, PowerBiProjectSummary,
    MAX_METADATA_BYTES,
};
use common::*;
use std::ffi::{CString, OsStr};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// Runs discovery on another thread so a regression that blocks (for
/// example on a FIFO) fails the test instead of hanging it.
fn discover_with_timeout(path: &Path) -> PowerBiProjectSummary {
    let path = path.to_path_buf();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(discover_project(&path));
    });
    rx.recv_timeout(Duration::from_secs(20))
        .expect("discovery blocked or panicked")
}

fn mkfifo(path: &Path) {
    let c_path = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c_path` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0, "mkfifo");
}

#[test]
fn report_reference_escaping_the_root_is_rejected() {
    let tmp = TempDir::new("escape-report");
    // A valid project exists outside the root; it must never be discovered.
    build_pbir_tmdl(tmp.path(), "Outside");
    let root = tmp.path().join("project");
    fs::create_dir(&root).unwrap();
    for reference in ["../Outside.Report", "a/../../Outside.Report", ".."] {
        write(&root.join("P.pbip"), pbip_json(reference));
        let summary = discover_project(&root.join("P.pbip"));
        assert_has(&summary, Code::ReferenceOutsideProject);
        assert_eq!(summary.report.path, None, "{reference}");
        assert!(!summary.report.exists);
        assert!(!serialized(&summary).contains("Outside.Report"));
    }
}

#[test]
fn model_reference_escaping_the_root_is_rejected() {
    let tmp = TempDir::new("escape-model");
    build_pbir_tmdl(tmp.path(), "Outside");
    let root = tmp.path().join("project");
    let pbip = build_pbir_tmdl(&root, "P");
    for reference in [
        "../../Outside.SemanticModel",
        "../../project/P.SemanticModel",
    ] {
        write(
            &root.join("P.Report/definition.pbir"),
            pbir_json("4.0", reference),
        );
        let summary = discover_project(&pbip);
        assert_eq!(summary.report.format, ComponentFormat::Pbir);
        assert_has(&summary, Code::ReferenceOutsideProject);
        assert_eq!(summary.semantic_model.path, None);
        assert_eq!(summary.semantic_model.format, ComponentFormat::Missing);
    }
}

#[test]
fn absolute_unc_and_drive_references_are_rejected() {
    let tmp = TempDir::new("absolute");
    let outside = tmp.path().join("outside");
    build_pbir_tmdl(&outside, "Abs");
    let root = tmp.path().join("project");
    let pbip = build_pbir_tmdl(&root, "P");

    let unix_absolute = outside
        .join("Abs.SemanticModel")
        .to_string_lossy()
        .into_owned();
    let references = [
        unix_absolute.as_str(),
        "//server/share/Model",
        "\\\\server\\share\\Model",
        "\\\\?\\C:\\Model",
        "C:\\Models\\Sales.SemanticModel",
        "C:/Models/Sales.SemanticModel",
        "D:Sales.SemanticModel",
        "file:///etc",
    ];
    for reference in references {
        write(
            &root.join("P.Report/definition.pbir"),
            pbir_json("4.0", reference),
        );
        let summary = discover_project(&pbip);
        assert_has(&summary, Code::ReferenceAbsolute);
        assert_eq!(summary.semantic_model.path, None, "{reference}");

        write(&pbip, pbip_json(reference));
        let summary = discover_project(&pbip);
        assert_has(&summary, Code::ReferenceAbsolute);
        assert_eq!(summary.report.path, None, "{reference}");
        write(&pbip, pbip_json("P.Report"));
    }

    // Backslash separators are not the documented form and are not guessed.
    write(
        &root.join("P.Report/definition.pbir"),
        pbir_json("4.0", "..\\P.SemanticModel"),
    );
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::SemanticModelReferenceInvalid);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Missing);
}

#[test]
fn symlinked_pbip_is_rejected() {
    let tmp = TempDir::new("link-pbip");
    let pbip = build_pbir_tmdl(tmp.path(), "Real");
    let link = tmp.path().join("Link.pbip");
    symlink(&pbip, &link).unwrap();
    let summary = discover_project(&link);
    assert_has(&summary, Code::SymlinkRejected);
    assert_eq!(summary.report.format, ComponentFormat::Missing);
}

#[test]
fn symlinked_component_folders_are_not_followed() {
    let tmp = TempDir::new("link-folders");
    let outside = tmp.path().join("outside");
    build_pbir_tmdl(&outside, "O");

    // Report folder is a symlink to a valid report outside the project.
    let root = tmp.path().join("r1");
    let pbip = build_pbir_tmdl(&root, "P");
    fs::remove_dir_all(root.join("P.Report")).unwrap();
    symlink(outside.join("O.Report"), root.join("P.Report")).unwrap();
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::SymlinkRejected);
    assert!(!summary.report.exists);
    assert_eq!(summary.report.format, ComponentFormat::Missing);

    // Model folder is a symlink, even to a sibling inside the project.
    let root = tmp.path().join("r2");
    let pbip = build_pbir_tmdl(&root, "P");
    fs::rename(
        root.join("P.SemanticModel"),
        root.join("Real.SemanticModel"),
    )
    .unwrap();
    symlink("Real.SemanticModel", root.join("P.SemanticModel")).unwrap();
    let summary = discover_project(&pbip);
    assert_eq!(summary.report.format, ComponentFormat::Pbir);
    assert_has(&summary, Code::SymlinkRejected);
    assert!(!summary.semantic_model.exists);

    // An intermediate path component is a symlink.
    let root = tmp.path().join("r3");
    fs::create_dir_all(&root).unwrap();
    symlink(&outside, root.join("via")).unwrap();
    write(&root.join("P.pbip"), pbip_json("via/O.Report"));
    let summary = discover_project(&root.join("P.pbip"));
    assert_has(&summary, Code::SymlinkRejected);
    assert!(!summary.report.exists);
    let diag = summary
        .diagnostics
        .iter()
        .find(|d| d.code == Code::SymlinkRejected)
        .unwrap();
    assert_eq!(
        diag.path.as_deref(),
        Some(root.join("via").to_string_lossy().as_ref())
    );
}

#[test]
fn symlinked_metadata_and_markers_are_not_followed() {
    let tmp = TempDir::new("link-files");
    let secret = tmp.path().join("secret.json");
    write(&secret, r#"{"version": "Password=hunter2"}"#);
    let mut n = 0;
    let mut project = |edit: &dyn Fn(&Path)| {
        n += 1;
        let root = tmp.path().join(format!("p{n}"));
        let pbip = build_pbir_tmdl(&root, "S");
        edit(&root);
        discover_project(&pbip)
    };
    let relink = |path: &Path, target: &Path| {
        if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
        symlink(target, path).unwrap();
    };

    let s = project(&|r| relink(&r.join("S.Report/definition.pbir"), &secret));
    assert_has(&s, Code::SymlinkRejected);
    assert_eq!(s.report.format, ComponentFormat::Unknown);

    let s = project(&|r| relink(&r.join("S.SemanticModel/definition.pbism"), &secret));
    assert_has(&s, Code::SymlinkRejected);
    assert_eq!(s.semantic_model.format, ComponentFormat::Unknown);

    let s = project(&|r| relink(&r.join("S.Report/definition/version.json"), &secret));
    assert_has(&s, Code::SymlinkRejected);
    assert_eq!(s.report.format, ComponentFormat::Unknown);

    let s = project(&|r| relink(&r.join("S.SemanticModel/definition/model.tmdl"), &secret));
    assert_has(&s, Code::SymlinkRejected);
    assert_eq!(s.semantic_model.format, ComponentFormat::Unknown);

    let s = project(&|r| {
        let real = r.join("real-definition");
        fs::rename(r.join("S.Report/definition"), &real).unwrap();
        symlink(&real, r.join("S.Report/definition")).unwrap();
    });
    assert_has(&s, Code::SymlinkRejected);
    assert_eq!(s.report.format, ComponentFormat::Unknown);

    let s = project(&|r| {
        fs::remove_dir_all(r.join("S.SemanticModel/definition")).unwrap();
        symlink(&secret, r.join("S.SemanticModel/model.bim")).unwrap();
    });
    assert_has(&s, Code::SymlinkRejected);
    assert_eq!(s.semantic_model.format, ComponentFormat::Unknown);

    // Dangling symlinks are rejected the same way, never treated as absent.
    let s = project(&|r| {
        relink(
            &r.join("S.Report/definition.pbir"),
            Path::new("/nonexistent/x"),
        )
    });
    assert_has(&s, Code::SymlinkRejected);
}

#[test]
fn symlinked_ancestor_of_selected_file_is_allowed() {
    let tmp = TempDir::new("link-ancestor");
    let real = tmp.path().join("real");
    build_pbir_tmdl(&real, "A");
    symlink(&real, tmp.path().join("alias")).unwrap();
    let summary = discover_project(&tmp.path().join("alias/A.pbip"));
    assert_eq!(summary.diagnostics, vec![]);
    assert_eq!(summary.report.format, ComponentFormat::Pbir);
}

#[test]
fn non_regular_metadata_is_rejected_without_blocking() {
    let tmp = TempDir::new("nonregular");

    let fifo_pbip = tmp.path().join("Fifo.pbip");
    mkfifo(&fifo_pbip);
    let summary = discover_with_timeout(&fifo_pbip);
    assert_has(&summary, Code::NotARegularFile);

    let root = tmp.path().join("fifo-pbir");
    let pbip = build_pbir_tmdl(&root, "F");
    fs::remove_file(root.join("F.Report/definition.pbir")).unwrap();
    mkfifo(&root.join("F.Report/definition.pbir"));
    let summary = discover_with_timeout(&pbip);
    assert_has(&summary, Code::NotARegularFile);
    assert_eq!(summary.report.format, ComponentFormat::Unknown);

    let root = tmp.path().join("dir-pbism");
    let pbip = build_pbir_tmdl(&root, "F");
    fs::remove_file(root.join("F.SemanticModel/definition.pbism")).unwrap();
    fs::create_dir(root.join("F.SemanticModel/definition.pbism")).unwrap();
    let summary = discover_with_timeout(&pbip);
    assert_has(&summary, Code::NotARegularFile);

    let root = tmp.path().join("fifo-marker");
    let pbip = build_pbir_tmdl(&root, "F");
    fs::remove_file(root.join("F.SemanticModel/definition/model.tmdl")).unwrap();
    mkfifo(&root.join("F.SemanticModel/definition/model.tmdl"));
    let summary = discover_with_timeout(&pbip);
    assert_has(&summary, Code::NotARegularFile);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Unknown);

    let root = tmp.path().join("socket-marker");
    let pbip = build_pbir_tmdl(&root, "F");
    fs::remove_dir_all(root.join("F.Report/definition")).unwrap();
    let _listener =
        std::os::unix::net::UnixListener::bind(root.join("F.Report/report.json")).unwrap();
    let summary = discover_with_timeout(&pbip);
    assert_has(&summary, Code::NotARegularFile);
    assert_eq!(summary.report.format, ComponentFormat::Unknown);

    let root = tmp.path().join("dir-marker");
    let pbip = build_pbir_tmdl(&root, "F");
    fs::remove_dir_all(root.join("F.SemanticModel/definition")).unwrap();
    fs::create_dir(root.join("F.SemanticModel/model.bim")).unwrap();
    let summary = discover_with_timeout(&pbip);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Unknown);

    let root = tmp.path().join("file-folder");
    let pbip = build_pbir_tmdl(&root, "F");
    fs::remove_dir_all(root.join("F.Report")).unwrap();
    write(&root.join("F.Report"), "not a folder");
    let summary = discover_with_timeout(&pbip);
    assert_has(&summary, Code::NotADirectory);
    assert!(!summary.report.exists);
}

#[test]
fn oversized_metadata_is_not_read() {
    let tmp = TempDir::new("large");
    let limit = usize::try_from(MAX_METADATA_BYTES).unwrap();

    // Exactly at the limit is accepted.
    let root = tmp.path().join("at-limit");
    let pbip = build_pbir_tmdl(&root, "L");
    let mut body = pbip_json("L.Report").into_bytes();
    body.resize(limit, b' ');
    write(&pbip, &body);
    assert_eq!(discover_project(&pbip).report.format, ComponentFormat::Pbir);

    // One byte over is rejected.
    body.push(b' ');
    write(&pbip, &body);
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::MetadataTooLarge);
    assert_eq!(summary.report.format, ComponentFormat::Missing);

    // A huge sparse definition file is rejected from its size alone.
    let root = tmp.path().join("sparse");
    let pbip = build_pbir_tmdl(&root, "L");
    fs::File::create(root.join("L.SemanticModel/definition.pbism"))
        .unwrap()
        .set_len(4 * 1024 * 1024 * 1024)
        .unwrap();
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::MetadataTooLarge);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Unknown);
}

#[test]
fn invalid_utf8_content_is_diagnosed() {
    let tmp = TempDir::new("utf8-content");
    for (index, file) in [
        "U.pbip",
        "U.Report/definition.pbir",
        "U.SemanticModel/definition.pbism",
    ]
    .iter()
    .enumerate()
    {
        let root = tmp.path().join(format!("p{index}"));
        let pbip = build_pbir_tmdl(&root, "U");
        write(&root.join(file), b"{\"version\": \"\xC3\x28\"}");
        let summary = discover_project(&pbip);
        assert_has(&summary, Code::MetadataInvalidUtf8);
    }
}

#[test]
fn invalid_utf8_project_path_is_supported_with_lossy_display() {
    let tmp = TempDir::new("utf8-path");
    let root = tmp.path().join(OsStr::from_bytes(b"proj-\xFF"));
    let pbip = build_pbir_tmdl(&root, "U");
    let summary = discover_project(&pbip);
    assert_eq!(summary.report.format, ComponentFormat::Pbir);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Tmdl);
    assert_has(&summary, Code::PathNotUtf8);
    assert!(summary.project_root.contains('\u{FFFD}'));
    assert!(!has_error(&summary));
}

#[test]
fn unreadable_metadata_is_diagnosed() {
    if is_root() {
        eprintln!("skipping: permission checks do not apply to root");
        return;
    }
    let tmp = TempDir::new("perms");
    let pbip = build_pbir_tmdl(tmp.path(), "D");
    let pbir = tmp.path().join("D.Report/definition.pbir");
    fs::set_permissions(&pbir, fs::Permissions::from_mode(0o000)).unwrap();
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::FileAccessDenied);
    assert_eq!(summary.report.format, ComponentFormat::Unknown);

    let model = tmp.path().join("D.SemanticModel");
    fs::set_permissions(&pbir, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&model, fs::Permissions::from_mode(0o000)).unwrap();
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::FileAccessDenied);
    fs::set_permissions(&model, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn diagnostics_never_echo_untrusted_values() {
    let tmp = TempDir::new("no-echo");
    let secret = "Password=hunter2;User=admin";
    let root = tmp.path().join("json");
    let pbip = build_pbir_tmdl(&root, "E");
    write(
        &pbip,
        format!("{{\"artifacts\": [{{\"report\": {{\"path\": \"{secret}\" oops"),
    );
    assert!(!serialized(&discover_project(&pbip)).contains("hunter2"));

    for reference in [
        format!("../../{secret}"),
        format!("/{secret}"),
        format!("C:\\{secret}"),
        format!("{secret}\\x"),
    ] {
        write(&pbip, pbip_json(&reference));
        assert!(!serialized(&discover_project(&pbip)).contains("hunter2"));
    }
}

#[test]
fn hostile_json_shapes_never_panic() {
    let tmp = TempDir::new("hostile");
    let deep = format!(
        "{{\"artifacts\": {}{}}}",
        "[".repeat(50_000),
        "]".repeat(50_000)
    );
    let pbip_cases = [
        "",
        "null",
        "\"x\"",
        "1e999999",
        "{",
        "{\"artifacts\": null}",
        "{\"artifacts\": [null, 1, \"x\", []]}",
        "{\"artifacts\": [{\"report\": null}]}",
        "{\"artifacts\": [{\"report\": {\"path\": [\"x\"]}}]}",
        "{\"artifacts\": [{\"report\": {\"path\": \"\\u0000\"}}]}",
        "{\"artifacts\": [{\"report\": {\"path\": \"a/\\u0000/b\"}}]}",
        "{\"$schema\": {}, \"version\": [], \"artifacts\": [{\"report\": {\"path\": \".\"}}]}",
        deep.as_str(),
    ];
    for (index, json) in pbip_cases.iter().enumerate() {
        let pbip = tmp.path().join(format!("h{index}.pbip"));
        write(&pbip, json);
        let summary = discover_project(&pbip);
        assert!(!summary.diagnostics.is_empty(), "case {index}");
    }

    let definition_cases = [
        "{\"datasetReference\": 5}",
        "{\"datasetReference\": {\"byPath\": 7}}",
        "{\"datasetReference\": {\"byConnection\": 7}}",
        "{\"version\": 4.0, \"datasetReference\": {\"byPath\": {\"path\": \"../\\u0007\"}}}",
    ];
    for (index, json) in definition_cases.iter().enumerate() {
        let root = tmp.path().join(format!("d{index}"));
        let pbip = build_pbir_tmdl(&root, "H");
        write(&root.join("H.Report/definition.pbir"), json);
        let summary = discover_project(&pbip);
        assert!(
            has_error(&summary) || !summary.diagnostics.is_empty(),
            "case {index}"
        );
    }
}

#[test]
fn report_folder_may_be_the_project_root() {
    // "." stays inside the root; the root itself is then the report folder.
    let tmp = TempDir::new("root-report");
    let root = tmp.path();
    write(&root.join("P.pbip"), pbip_json("."));
    write(&root.join("definition.pbir"), pbir_json("4.0", "M"));
    write(&root.join("report.json"), "{}");
    write(&root.join("M/definition.pbism"), pbism_json("4.0"));
    write(&root.join("M/model.bim"), "{}");
    let summary = discover_project(&root.join("P.pbip"));
    assert_eq!(summary.report.format, ComponentFormat::PbirLegacy);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Tmsl);
}
