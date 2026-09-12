//! Integration tests for the WP02 safe-write API
//! (`ProjectSession`/`ProjectTransaction`): every one of the committed
//! preservation fixtures is opened and saved through a disposable copy
//! only — fixture originals under `fixtures/powerbi` are never opened for
//! writing.
//!
//! Default-feature tests below exercise the production surface reachable
//! without `safe_write_test_support`. The `test_support` module at the
//! bottom exercises the hidden staging/failure-injection hooks and only
//! compiles when that feature is enabled
//! (`cargo test -p biforgeworks-powerbi --features safe_write_test_support`).

#![cfg(target_os = "linux")]

mod common;
mod preservation_support;

use biforgeworks_powerbi::safe_writes::{
    ProjectSession, ProjectState, SafeWriteErrorKind, StageOutcome, JOURNAL_DIR,
};
use biforgeworks_powerbi::snapshot::FileCategory;
use biforgeworks_powerbi::ComponentFormat;
use common::{build_pbir_tmdl, collect_paths, copy_fixture, set_stale_times, write, TempDir};
use preservation_support::{assert_preserved, capture, compare};
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURES: &[&str] = &[
    "preservation-basic",
    "preservation-unknown-files",
    "preservation-unknown-json",
    "preservation-crlf",
    "external-change",
    "multi-file-rollback",
];

/// The single `.pbip` a freshly copied fixture directory contains.
fn pbip_in(dir: &Path) -> PathBuf {
    fs::read_dir(dir)
        .expect("read fixture dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "pbip"))
        .expect("fixture has exactly one .pbip file")
}

/// Copies `fixture` into a fresh subdirectory of `tmp` and returns its
/// directory and `.pbip` path.
fn open_fixture(fixture: &str, tmp: &Path) -> (PathBuf, PathBuf) {
    let dir = copy_fixture(fixture, tmp);
    let pbip = pbip_in(&dir);
    (dir, pbip)
}

fn pbip_name(pbip: &Path) -> String {
    pbip.file_name().unwrap().to_string_lossy().into_owned()
}

/// Asserts the only difference between `before` and `after` is that the
/// bytes at the first differing position read `from` in `before` and `to`
/// in `after`, with everything following identical in both — used to
/// confirm an edit changed exactly one JSON boolean token and nothing
/// else, regardless of surrounding BOM, CRLF, indentation, or unrelated
/// properties.
fn assert_minimal_token_flip(before: &[u8], after: &[u8], from: &[u8], to: &[u8]) {
    let mut prefix = 0;
    while prefix < before.len() && prefix < after.len() && before[prefix] == after[prefix] {
        prefix += 1;
    }
    assert!(
        before[prefix..].starts_with(from),
        "expected {from:?} at the first differing byte (offset {prefix}), found {:?}",
        &before[prefix..(prefix + from.len()).min(before.len())]
    );
    assert!(
        after[prefix..].starts_with(to),
        "expected {to:?} at the first differing byte (offset {prefix}), found {:?}",
        &after[prefix..(prefix + to.len()).min(after.len())]
    );
    assert_eq!(
        &before[prefix + from.len()..],
        &after[prefix + to.len()..],
        "bytes after the changed token must match exactly"
    );
}

// ---------------------------------------------------------------------
// A: every fixture discovers, opens, and snapshots cleanly.
// ---------------------------------------------------------------------

#[test]
fn every_fixture_opens_and_snapshots_with_expected_categories() {
    let tmp = TempDir::new("safe-writes-open");
    for fixture in FIXTURES {
        let (_dir, pbip) = open_fixture(fixture, tmp.path());
        let session = ProjectSession::open(&pbip)
            .unwrap_or_else(|error| panic!("{fixture}: session should open: {error:?}"));
        assert_eq!(session.state(), ProjectState::Clean, "{fixture}");
        assert!(
            session.summary().diagnostics.is_empty(),
            "{fixture}: {:?}",
            session.summary().diagnostics
        );
        assert_eq!(
            session.summary().report.format,
            ComponentFormat::Pbir,
            "{fixture}"
        );
        assert_eq!(
            session.summary().semantic_model.format,
            ComponentFormat::Tmdl,
            "{fixture}"
        );

        let snapshot = session.snapshot();
        assert!(snapshot.file_count > 0, "{fixture}");
        assert!(snapshot.directory_count > 0, "{fixture}");
        assert_eq!(
            snapshot.entries.len(),
            snapshot.file_count + snapshot.directory_count,
            "{fixture}"
        );

        let managed: Vec<_> = snapshot
            .entries
            .iter()
            .filter(|entry| entry.category == FileCategory::Managed)
            .collect();
        assert_eq!(managed.len(), 1, "{fixture}: exactly one managed file");
        assert_eq!(managed[0].path, pbip_name(&pbip), "{fixture}");

        assert!(
            snapshot
                .entries
                .iter()
                .any(|entry| entry.category == FileCategory::Preserved),
            "{fixture}: expected at least one preserved entry"
        );
        assert!(
            snapshot
                .entries
                .iter()
                .any(|entry| entry.category == FileCategory::Unknown),
            "{fixture}: expected at least one unknown entry (the semantic model's own tmdl files)"
        );
    }
}

// ---------------------------------------------------------------------
// B: staging the value already on disk is a true no-op, even under
// deliberately stale timestamps.
// ---------------------------------------------------------------------

#[test]
fn staging_the_current_value_is_a_no_op_under_stale_timestamps() {
    let tmp = TempDir::new("safe-writes-noop");
    for fixture in FIXTURES {
        let (dir, pbip) = open_fixture(fixture, tmp.path());
        let paths = collect_paths(&dir);
        set_stale_times(&paths);
        let before = capture(&dir);

        let mut session = ProjectSession::open(&pbip).unwrap();
        let mut tx = session.begin_transaction().unwrap();
        // Every fixture's committed value is `true`.
        assert_eq!(
            tx.stage_auto_recovery(true).unwrap(),
            StageOutcome::AlreadyMatches,
            "{fixture}"
        );
        assert!(tx.staged().is_empty(), "{fixture}");
        let receipt = tx.commit().unwrap_or_else(|e| panic!("{fixture}: {e:?}"));
        assert!(receipt.unchanged, "{fixture}");
        assert!(receipt.changed_files.is_empty(), "{fixture}");
        assert_eq!(session.state(), ProjectState::Clean, "{fixture}");

        // A true no-op: no writes at all, so nothing may drift, not even
        // the project root's own bookkeeping timestamps.
        let after = capture(&dir);
        assert_preserved(&before, &after, &[], &[], &[]);
        assert!(
            !dir.join(JOURNAL_DIR).exists(),
            "{fixture}: no-op must create no journal"
        );
    }
}

// ---------------------------------------------------------------------
// C/D/E: a real edit changes exactly the boolean token in the managed
// `.pbip` and preserves everything else byte-for-byte — unknown files,
// unknown JSON properties (including inside the rewritten document
// itself), and BOM/CRLF/no-trailing-newline encodings included.
// ---------------------------------------------------------------------

#[test]
fn commit_changes_only_the_boolean_token_and_preserves_everything_else() {
    let tmp = TempDir::new("safe-writes-edit");
    for fixture in FIXTURES {
        let (dir, pbip) = open_fixture(fixture, tmp.path());
        let original = fs::read(&pbip).unwrap();
        let before = capture(&dir);

        let mut session = ProjectSession::open(&pbip).unwrap();
        let mut tx = session.begin_transaction().unwrap();
        assert_eq!(
            tx.stage_auto_recovery(false).unwrap(),
            StageOutcome::Changed,
            "{fixture}"
        );
        let receipt = tx.commit().unwrap_or_else(|e| panic!("{fixture}: {e:?}"));
        assert!(!receipt.unchanged, "{fixture}");
        assert_eq!(receipt.changed_files, vec![pbip_name(&pbip)], "{fixture}");

        let updated = fs::read(&pbip).unwrap();
        assert_ne!(updated, original, "{fixture}");
        assert_minimal_token_flip(&original, &updated, b"true", b"false");

        // The pbip lives directly at the project root in every fixture, so
        // its own rewrite (and the journal directory's create/remove) is
        // the root's only permitted bookkeeping.
        let after = capture(&dir);
        let managed = pbip_name(&pbip);
        assert_preserved(&before, &after, &[managed.as_str()], &[""], &[]);
        assert!(
            !dir.join(JOURNAL_DIR).exists(),
            "{fixture}: a clean commit must not leave a journal"
        );
    }
}

// ---------------------------------------------------------------------
// F: external changes to managed, preserved, or unknown content between
// open and commit are detected and block the save without writing
// anything.
// ---------------------------------------------------------------------

#[test]
fn staging_refuses_when_the_managed_file_changed_since_open() {
    let tmp = TempDir::new("safe-writes-managed-conflict");
    let (_dir, pbip) = open_fixture("external-change", tmp.path());
    let mut session = ProjectSession::open(&pbip).unwrap();

    // Whitespace-only, but still a byte-for-byte change the snapshot must
    // catch: this crate never treats "semantically equivalent" as
    // "unchanged".
    let mut bytes = fs::read(&pbip).unwrap();
    bytes.push(b'\n');
    fs::write(&pbip, &bytes).unwrap();

    let mut tx = session.begin_transaction().unwrap();
    let error = tx
        .stage_auto_recovery(false)
        .expect_err("staging must refuse a managed file that changed since open");
    assert_eq!(error.kind, SafeWriteErrorKind::Conflict);
}

type ConflictScenario = (&'static str, fn(&Path));

#[test]
fn external_changes_block_commit_and_leave_the_tree_untouched() {
    let scenarios: [ConflictScenario; 7] = [
        ("unknown file content changed", |dir| {
            let target = dir.join("External.Report/notes.txt");
            fs::write(target, b"mutated by an external actor").unwrap();
        }),
        ("preserved file content changed", |dir| {
            let target = dir.join("External.Report/definition/version.json");
            fs::write(target, br#"{"$schema":"x","version":"9.9.9"}"#).unwrap();
        }),
        ("unknown file deleted", |dir| {
            fs::remove_file(dir.join("External.Report/notes.txt")).unwrap();
        }),
        (
            "unknown file replaced with identical bytes (new inode)",
            |dir| {
                let target = dir.join("External.Report/notes.txt");
                let original = fs::read(&target).unwrap();
                let temp = dir.join("External.Report/notes.txt.tmp");
                fs::write(&temp, &original).unwrap();
                fs::rename(&temp, &target).unwrap();
            },
        ),
        ("unknown file type changed to a directory", |dir| {
            let target = dir.join("External.Report/notes.txt");
            fs::remove_file(&target).unwrap();
            fs::create_dir(&target).unwrap();
        }),
        ("unknown file added", |dir| {
            fs::write(dir.join("External.Report/unexpected.txt"), b"surprise").unwrap();
        }),
        ("managed .pbip changed after staging", |dir| {
            let pbip = pbip_in(dir);
            let mut bytes = fs::read(&pbip).unwrap();
            bytes.push(b'\n');
            fs::write(&pbip, &bytes).unwrap();
        }),
    ];

    for (label, mutate) in scenarios {
        // A fresh temporary directory per scenario: several mutations
        // change a file's type or remove it, so a shared, reused fixture
        // copy would carry stale state (or fail to be recopied) into the
        // next iteration.
        let tmp = TempDir::new("safe-writes-conflict");
        let (dir, pbip) = open_fixture("external-change", tmp.path());
        let mut session = ProjectSession::open(&pbip).unwrap();
        let mut tx = session.begin_transaction().unwrap();
        assert_eq!(
            tx.stage_auto_recovery(false).unwrap(),
            StageOutcome::Changed,
            "{label}"
        );

        mutate(&dir);
        let before_commit = capture(&dir);

        let error = tx.commit().expect_err(label);
        assert_eq!(
            error.kind,
            SafeWriteErrorKind::Conflict,
            "{label}: {error:?}"
        );
        assert!(!error.diagnostics.is_empty(), "{label}");

        let after_commit = capture(&dir);
        let violations = compare(&before_commit, &after_commit, &[], &[], &[]);
        assert!(
            violations.is_empty(),
            "{label}: a blocked commit must not touch the tree: {violations:?}"
        );
        assert!(
            !dir.join(JOURNAL_DIR).exists(),
            "{label}: a blocked commit must not leave a journal"
        );
    }
}

// ---------------------------------------------------------------------
// G: a fresh snapshot after a successful commit supports a second save.
// ---------------------------------------------------------------------

#[test]
fn a_fresh_snapshot_after_commit_supports_a_second_save() {
    let tmp = TempDir::new("safe-writes-second-save");
    let (dir, pbip) = open_fixture("preservation-basic", tmp.path());
    let original = fs::read(&pbip).unwrap();

    let mut session = ProjectSession::open(&pbip).unwrap();
    {
        let mut tx = session.begin_transaction().unwrap();
        assert_eq!(
            tx.stage_auto_recovery(false).unwrap(),
            StageOutcome::Changed
        );
        let receipt = tx.commit().unwrap();
        assert!(!receipt.unchanged);
    }
    assert_eq!(session.state(), ProjectState::Clean);

    {
        let mut tx = session
            .begin_transaction()
            .expect("a second transaction must begin after the refreshed snapshot");
        assert_eq!(tx.stage_auto_recovery(true).unwrap(), StageOutcome::Changed);
        let receipt = tx.commit().unwrap();
        assert!(!receipt.unchanged);
    }

    let restored = fs::read(&pbip).unwrap();
    assert_eq!(
        restored, original,
        "toggling the flag back should restore the exact original bytes"
    );
    assert!(!dir.join(JOURNAL_DIR).exists());
}

// ---------------------------------------------------------------------
// H: a missing or non-boolean `settings.enableAutoRecovery` is rejected
// before anything is written.
// ---------------------------------------------------------------------

#[test]
fn missing_or_non_boolean_auto_recovery_is_rejected_without_writing() {
    let tmp = TempDir::new("safe-writes-bad-setting");

    // `settings` absent entirely (what `common::build_pbir_tmdl` produces).
    let root = tmp.path().join("missing-settings");
    let pbip = build_pbir_tmdl(&root, "Missing");
    let before = fs::read(&pbip).unwrap();
    let mut session = ProjectSession::open(&pbip).unwrap();
    let mut tx = session.begin_transaction().unwrap();
    let error = tx
        .stage_auto_recovery(false)
        .expect_err("a project with no settings object should be rejected");
    assert_eq!(error.kind, SafeWriteErrorKind::StageRejected);
    assert!(tx.staged().is_empty());
    drop(tx);
    assert_eq!(fs::read(&pbip).unwrap(), before);

    // `enableAutoRecovery` present but not a boolean.
    let root = tmp.path().join("non-bool-setting");
    let pbip = build_pbir_tmdl(&root, "NonBool");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&pbip).unwrap()).unwrap();
    document["settings"] = serde_json::json!({ "enableAutoRecovery": "true" });
    write(&pbip, serde_json::to_vec_pretty(&document).unwrap());
    let before = fs::read(&pbip).unwrap();

    let mut session = ProjectSession::open(&pbip).unwrap();
    let mut tx = session.begin_transaction().unwrap();
    let error = tx
        .stage_auto_recovery(false)
        .expect_err("a non-boolean enableAutoRecovery should be rejected");
    assert_eq!(error.kind, SafeWriteErrorKind::StageRejected);
    drop(tx);
    assert_eq!(fs::read(&pbip).unwrap(), before);
}

// ---------------------------------------------------------------------
// A leftover journal directory is reported as pending recovery on open,
// and transactions are refused until it is resolved.
// ---------------------------------------------------------------------

#[test]
fn a_leftover_journal_directory_is_reported_as_pending_recovery() {
    let tmp = TempDir::new("safe-writes-pending-recovery");
    let (dir, pbip) = open_fixture("preservation-basic", tmp.path());

    let journal_dir = dir.join(JOURNAL_DIR);
    fs::create_dir(&journal_dir).unwrap();
    fs::create_dir(journal_dir.join("originals")).unwrap();
    let record = serde_json::json!({
        "format": "biforgeworks-save-journal",
        "version": 1,
        "phase": "replacing",
        "project_root": dir.to_string_lossy(),
        "targets": [{
            "path": "Preserve.pbip",
            "original_file": "originals/0000",
            "original_sha256": "0".repeat(64),
            "new_sha256": "1".repeat(64),
            "replaced": true,
        }],
    });
    write(&journal_dir.join("journal.json"), record.to_string());

    let mut session = ProjectSession::open(&pbip).expect("open still succeeds for inspection");
    assert_eq!(session.state(), ProjectState::Error);
    let recovery = session
        .pending_recovery()
        .expect("recovery artifacts should be detected");
    assert!(!recovery.journal_unreadable);
    assert_eq!(recovery.targets.len(), 1);
    assert_eq!(recovery.targets[0].path, "Preserve.pbip");
    assert!(recovery.targets[0].replaced);

    // `ProjectTransaction` is not `Debug`, so `Result::expect_err` cannot be
    // used on `begin_transaction`'s result.
    let Err(error) = session.begin_transaction() else {
        panic!("transactions must be refused while recovery is pending");
    };
    assert_eq!(error.kind, SafeWriteErrorKind::RecoveryPending);
}

// ---------------------------------------------------------------------
// Demonstration: run with `--nocapture` to see the evidence directly.
// ---------------------------------------------------------------------

#[test]
fn demonstration_walkthrough_prints_evidence() {
    let tmp = TempDir::new("safe-writes-demo");
    let (dir, pbip) = open_fixture("preservation-unknown-files", tmp.path());

    let mut session = ProjectSession::open(&pbip).expect("open");
    {
        let snapshot = session.snapshot();
        println!("fixture path: {}", pbip.display());
        println!(
            "entries: {} total ({} files, {} directories)",
            snapshot.entries.len(),
            snapshot.file_count,
            snapshot.directory_count
        );
        let managed: Vec<_> = snapshot
            .entries
            .iter()
            .filter(|e| e.category == FileCategory::Managed)
            .map(|e| e.path.clone())
            .collect();
        let preserved: Vec<_> = snapshot
            .entries
            .iter()
            .filter(|e| e.category == FileCategory::Preserved)
            .map(|e| e.path.clone())
            .collect();
        println!("managed: {managed:?}");
        println!("preserved: {preserved:?}");
        println!(
            "report format: {:?}, model format: {:?}",
            session.summary().report.format,
            session.summary().semantic_model.format
        );
    }

    // Successful commit: capture immediately around it and print/assert
    // that nothing beyond the managed file moved.
    let before_success = capture(&dir);
    let mut tx = session.begin_transaction().unwrap();
    tx.stage_auto_recovery(false).unwrap();
    let receipt = tx.commit().expect("commit succeeds");
    println!(
        "commit result: changed_files={:?} unchanged={}",
        receipt.changed_files, receipt.unchanged
    );
    assert!(!receipt.unchanged);
    let after_success = capture(&dir);
    let pbip_rel = pbip_name(&pbip);
    let success_violations = compare(
        &before_success,
        &after_success,
        &[pbip_rel.as_str()],
        &[""],
        &[],
    );
    println!(
        "unexpected modifications from the successful commit: {} ({:?})",
        success_violations.len(),
        success_violations
    );
    assert!(success_violations.is_empty());

    // Blocked commit: mutate an existing unknown file (EXTERNAL_FILE_CHANGED)
    // and show the tree is left exactly as that mutation left it.
    let target = dir.join("TempFiles/scratch.opaque");
    fs::write(&target, b"mutated between open and commit").unwrap();
    let mut tx = session.begin_transaction().unwrap();
    tx.stage_auto_recovery(true).unwrap();
    let before_blocked = capture(&dir);
    let error = tx
        .commit()
        .expect_err("external content change should block the commit");
    println!("conflict diagnostics: {:?}", error.diagnostics);
    let after_blocked = capture(&dir);
    let blocked_violations = compare(&before_blocked, &after_blocked, &[], &[], &[]);
    println!(
        "unexpected modifications from the blocked commit: {} ({:?})",
        blocked_violations.len(),
        blocked_violations
    );
    assert!(blocked_violations.is_empty());
}

// ---------------------------------------------------------------------
// Test-support hooks (`safe_write_test_support` feature only): staged
// non-`.pbip` replacements and deterministic failure injection.
// ---------------------------------------------------------------------

#[cfg(feature = "safe_write_test_support")]
mod test_support {
    use super::*;
    use biforgeworks_powerbi::safe_writes::FailurePoint;

    #[test]
    fn rollback_restores_every_replaced_file_after_a_mid_commit_failure() {
        let tmp = TempDir::new("safe-writes-rollback");
        let (dir, pbip) = open_fixture("multi-file-rollback", tmp.path());
        let probe = "Multi.SemanticModel/definition/tables/rollback-probe.tmdl";
        let before = capture(&dir);

        let mut session = ProjectSession::open(&pbip).unwrap();
        let mut tx = session.begin_transaction().unwrap();
        assert_eq!(
            tx.stage_auto_recovery(false).unwrap(),
            StageOutcome::Changed
        );
        assert_eq!(
            tx.stage_replacement_for_tests(probe, b"replaced-for-rollback-test\n".to_vec())
                .unwrap(),
            StageOutcome::Changed
        );
        tx.inject_failure(FailurePoint::AfterFirstReplace);
        let error = tx
            .commit()
            .expect_err("the injected failure should abort the commit");
        assert_eq!(error.kind, SafeWriteErrorKind::RolledBack);
        assert_eq!(session.state(), ProjectState::Error);

        // Captured via the same O_NOATIME helper used for `before`, so
        // reading it for the content assertions below does not itself
        // perturb the very timestamps `assert_preserved` checks next.
        let after = capture(&dir);

        // Content must be restored exactly, for both the file rollback
        // actually replaced and the pbip whose own turn never came.
        assert_eq!(
            after.contents[probe], before.contents[probe],
            "rollback must restore the staged target's exact original bytes"
        );
        assert_eq!(
            after.contents[&pbip_name(&pbip)],
            before.contents[&pbip_name(&pbip)],
            "a write that never started must leave the file untouched"
        );

        // A replace-then-rename mints a new inode even when it restores the
        // original bytes, so the restored target's own metadata (and the
        // root's and its parent directory's bookkeeping) are the only
        // permitted differences; everything else, content included, is
        // still checked exactly.
        assert_preserved(
            &before,
            &after,
            &[],
            &["", "Multi.SemanticModel/definition/tables"],
            &[probe],
        );
        assert!(
            !dir.join(JOURNAL_DIR).exists(),
            "a fully rolled-back save should clean up its own journal"
        );
    }

    #[test]
    fn preserved_files_are_rejected_as_unsafe_write_targets_even_via_test_support() {
        let tmp = TempDir::new("safe-writes-unsafe-target");
        let (_dir, pbip) = open_fixture("preservation-basic", tmp.path());

        let mut session = ProjectSession::open(&pbip).unwrap();
        let mut tx = session.begin_transaction().unwrap();
        let preserved_path = "Preserve.Report/definition/report.json";
        assert_eq!(
            tx.stage_replacement_for_tests(preserved_path, b"{}".to_vec())
                .unwrap(),
            StageOutcome::Changed
        );
        let error = tx
            .validate()
            .expect_err("staging a preserved file must fail validation");
        assert_eq!(error.kind, SafeWriteErrorKind::ValidationFailed);
    }
}
