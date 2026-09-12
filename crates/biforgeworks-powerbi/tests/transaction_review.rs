//! Independent regression coverage for integration-review findings.
#![cfg(target_os = "linux")]

mod common;

use biforgeworks_powerbi::safe_writes::{ProjectSession, ProjectState, JOURNAL_DIR};
use common::{build_pbir_tmdl, read_contents, write, TempDir};
use std::fs;
use std::path::{Path, PathBuf};

fn project(root: &Path) -> PathBuf {
    let pbip = build_pbir_tmdl(root, "Review");
    let original = contents(&pbip);
    let mut json: serde_json::Value = serde_json::from_slice(&original).unwrap();
    json["settings"] = serde_json::json!({"enableAutoRecovery": true});
    write(&pbip, serde_json::to_vec_pretty(&json).unwrap());
    pbip
}

fn contents(path: &Path) -> Vec<u8> {
    read_contents(&[path.to_path_buf()]).remove(path).unwrap()
}

#[test]
fn staging_back_to_original_discards_the_previous_edit() {
    let temp = TempDir::new("review-stage-revert");
    let pbip = project(temp.path());
    let before = contents(&pbip);
    let mut session = ProjectSession::open(&pbip).unwrap();
    let mut transaction = session.begin_transaction().unwrap();
    transaction.stage_auto_recovery(false).unwrap();
    transaction.stage_auto_recovery(true).unwrap();
    assert!(transaction.staged().is_empty());
    let receipt = transaction.commit().unwrap();
    assert!(receipt.unchanged);
    assert_eq!(contents(&pbip), before);
    assert!(!temp.path().join(JOURNAL_DIR).exists());
    assert_eq!(session.state(), ProjectState::Clean);
}

#[test]
fn cancelling_or_dropping_edits_leaves_no_phantom_dirty_state() {
    let temp = TempDir::new("review-cancel");
    let pbip = project(temp.path());
    let before = contents(&pbip);
    let mut session = ProjectSession::open(&pbip).unwrap();
    let mut transaction = session.begin_transaction().unwrap();
    transaction.stage_auto_recovery(false).unwrap();
    transaction.cancel();
    assert_eq!(session.state(), ProjectState::Clean);
    {
        let mut transaction = session.begin_transaction().unwrap();
        transaction.stage_auto_recovery(false).unwrap();
    }
    assert_eq!(session.state(), ProjectState::Clean);
    assert_eq!(contents(&pbip), before);
}

#[test]
fn replacing_selected_root_blocks_writes_to_both_old_and_new_roots() {
    let temp = TempDir::new("review-root-replaced");
    let root = temp.path().join("project");
    let pbip = project(&root);
    let before = contents(&pbip);
    let mut session = ProjectSession::open(&pbip).unwrap();
    let mut transaction = session.begin_transaction().unwrap();
    transaction.stage_auto_recovery(false).unwrap();
    let moved = temp.path().join("moved");
    fs::rename(&root, &moved).unwrap();
    let replacement = project(&root);
    let replacement_before = contents(&replacement);
    assert!(transaction.commit().is_err());
    assert_eq!(contents(&moved.join("Review.pbip")), before);
    assert_eq!(contents(&replacement), replacement_before);
    assert!(!moved.join(JOURNAL_DIR).exists());
    assert!(!root.join(JOURNAL_DIR).exists());
    assert!(session.refresh().is_err());
}

#[test]
fn new_recovery_artifacts_are_not_silently_ignored_by_noop() {
    let temp = TempDir::new("review-late-recovery");
    let pbip = project(temp.path());
    let before = contents(&pbip);
    let mut session = ProjectSession::open(&pbip).unwrap();
    let journal = temp.path().join(JOURNAL_DIR);
    write(&journal.join("unknown-evidence"), b"must remain untouched");
    let transaction = session.begin_transaction().unwrap();
    assert!(transaction.commit().is_err());
    assert_eq!(contents(&pbip), before);
    assert_eq!(
        contents(&journal.join("unknown-evidence")),
        b"must remain untouched"
    );
}

#[test]
fn missing_or_unsupported_components_cannot_be_saved() {
    for mutation in ["missing", "unknown", "escape"] {
        let temp = TempDir::new("review-invalid-base");
        let pbip = project(temp.path());
        let marker = temp.path().join("Review.Report/definition.pbir");
        match mutation {
            "missing" => fs::remove_file(&marker).unwrap(),
            "unknown" => {
                fs::remove_file(temp.path().join("Review.Report/definition/report.json")).unwrap();
            }
            "escape" => {
                let bytes = contents(&marker);
                let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                json["datasetReference"]["byPath"]["path"] = "../../outside".into();
                write(&marker, serde_json::to_vec(&json).unwrap());
            }
            _ => unreachable!(),
        }
        let before = contents(&pbip);
        if let Ok(mut session) = ProjectSession::open(&pbip) {
            if let Ok(mut transaction) = session.begin_transaction() {
                if transaction.stage_auto_recovery(false).is_ok() {
                    assert!(transaction.commit().is_err(), "saved {mutation} project");
                }
            }
        }
        assert_eq!(contents(&pbip), before);
        assert!(!temp.path().join(JOURNAL_DIR).exists());
    }
}

#[test]
fn a_conflict_found_while_staging_remains_visible_after_dropping_the_transaction() {
    let temp = TempDir::new("review-stage-conflict");
    let pbip = project(temp.path());
    let mut session = ProjectSession::open(&pbip).unwrap();
    let mut transaction = session.begin_transaction().unwrap();
    transaction.stage_auto_recovery(false).unwrap();
    let mut external = contents(&pbip);
    external.push(b'\n');
    write(&pbip, &external);
    assert!(transaction.stage_auto_recovery(true).is_err());
    drop(transaction);
    assert_eq!(session.state(), ProjectState::Conflict);
    assert_eq!(contents(&pbip), external);
}

#[test]
fn snapshots_fail_closed_on_oversized_files_and_non_utf8_names() {
    use biforgeworks_powerbi::snapshot::MAX_SNAPSHOT_FILE_BYTES;
    use biforgeworks_powerbi::DiagnosticCode;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    for oversized in [true, false] {
        let temp = TempDir::new("review-snapshot-limits");
        let pbip = project(temp.path());
        let before = contents(&pbip);
        let expected = if oversized {
            fs::File::create(temp.path().join("opaque-large.bin"))
                .unwrap()
                .set_len(MAX_SNAPSHOT_FILE_BYTES + 1)
                .unwrap();
            DiagnosticCode::SnapshotTooLarge
        } else {
            write(
                &temp.path().join(OsStr::from_bytes(b"opaque-\xff")),
                b"opaque",
            );
            DiagnosticCode::PathNotUtf8
        };
        let error = ProjectSession::open(&pbip).unwrap_err();
        assert!(error.diagnostics.iter().any(|d| d.code == expected));
        assert_eq!(contents(&pbip), before);
        assert!(!temp.path().join(JOURNAL_DIR).exists());
    }
}

#[cfg(feature = "safe_write_test_support")]
#[test]
fn test_staging_cannot_escape_or_follow_a_symlink() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("review-unsafe-staging");
    let root = temp.path().join("project");
    let pbip = project(&root);
    let outside = temp.path().join("outside.txt");
    write(&outside, b"outside sentinel");
    symlink(&outside, root.join("escape")).unwrap();
    let before = contents(&pbip);
    let mut session = ProjectSession::open(&pbip).unwrap();
    for path in [
        "",
        "../outside.txt",
        "./Review.pbip",
        "escape",
        "/outside.txt",
        "Review.Report/../Review.pbip",
    ] {
        let mut transaction = session.begin_transaction().unwrap();
        assert!(
            transaction
                .stage_replacement_for_tests(path, b"changed".to_vec())
                .is_err(),
            "accepted {path}"
        );
    }
    assert_eq!(contents(&outside), b"outside sentinel");
    assert_eq!(contents(&pbip), before);
    assert!(!root.join(JOURNAL_DIR).exists());
}

#[cfg(feature = "safe_write_test_support")]
#[test]
fn public_failure_hook_retains_recoverable_originals_and_blocks_reopening_for_write() {
    use biforgeworks_powerbi::safe_writes::{FailurePoint, SafeWriteErrorKind};

    let temp = TempDir::new("review-recovery-hook");
    let pbip = project(temp.path());
    let before = contents(&pbip);
    let mut session = ProjectSession::open(&pbip).unwrap();
    let mut transaction = session.begin_transaction().unwrap();
    transaction.stage_auto_recovery(false).unwrap();
    transaction.inject_failure(FailurePoint::RollbackRestore);
    let error = transaction.commit().unwrap_err();
    assert_eq!(error.kind, SafeWriteErrorKind::RecoveryRequired);
    assert_eq!(
        contents(&temp.path().join(JOURNAL_DIR).join("originals/0000")),
        before
    );
    let mut reopened = ProjectSession::open(&pbip).unwrap();
    assert!(reopened.pending_recovery().is_some());
    assert_eq!(reopened.state(), ProjectState::Error);
    assert_eq!(
        reopened.begin_transaction().unwrap_err().kind,
        SafeWriteErrorKind::RecoveryPending
    );
}
