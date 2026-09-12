#![cfg(target_os = "linux")]
mod common;
use biforgeworks_powerbi::tmdl::{inspect_project, model::InspectionStatus};
use common::*;
use std::os::unix::fs::symlink;

#[test]
fn source_symlinks_and_directory_symlinks_are_rejected() {
    for directory in [false, true] {
        let tmp = TempDir::new("tmdl-symlink");
        let dir = copy_fixture("tmdl-star-schema", tmp.path());
        let definition = dir.join("Sales.SemanticModel/definition");
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        write(&outside.join("secret.tmdl"), "table DoNotRead");
        let target = if directory {
            outside.clone()
        } else {
            outside.join("secret.tmdl")
        };
        symlink(
            target,
            definition.join(if directory { "extra" } else { "extra.tmdl" }),
        )
        .unwrap();
        let r = inspect_project(&dir.join("Sales.pbip"));
        assert!(matches!(r.status, InspectionStatus::Error));
        assert!(r.model.is_none());
    }
}
#[test]
fn invalid_utf8_and_oversized_sources_fail_closed() {
    let tmp = TempDir::new("tmdl-size");
    let dir = copy_fixture("tmdl-minimal", tmp.path());
    let source = dir.join("Sales.SemanticModel/definition/bad.tmdl");
    write(&source, [0xff]);
    assert!(matches!(
        inspect_project(&dir.join("Sales.pbip")).status,
        InspectionStatus::Error
    ));
    let file = std::fs::File::create(&source).unwrap();
    file.set_len((biforgeworks_powerbi::tmdl::parser::MAX_SOURCE_BYTES + 1) as u64)
        .unwrap();
    assert!(matches!(
        inspect_project(&dir.join("Sales.pbip")).status,
        InspectionStatus::Error
    ));
}
#[test]
fn tmdl_scripts_are_not_loaded_and_snapshot_categories_do_not_change() {
    let tmp = TempDir::new("tmdl-scripts");
    let dir = copy_fixture("tmdl-star-schema", tmp.path());
    write(
        &dir.join("Sales.SemanticModel/TMDLScripts/hostile.tmdl"),
        "createOrReplace\n\ttable NotAModelObject\n",
    );
    let pbip = dir.join("Sales.pbip");
    let before = biforgeworks_powerbi::safe_writes::ProjectSession::open(&pbip).unwrap();
    let r = inspect_project(&pbip);
    assert_eq!(r.model.unwrap().tables.len(), 2);
    assert!(r.documents.iter().all(|d| !d.file.contains("TMDLScripts")));
    let after = biforgeworks_powerbi::safe_writes::ProjectSession::open(&pbip).unwrap();
    assert_eq!(before.snapshot().entries, after.snapshot().entries);
}
