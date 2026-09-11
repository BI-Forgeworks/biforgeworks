//! Discovery behavior: fixtures, reference resolution, formats, missing and
//! malformed metadata, unsupported versions, and the serialized contract.

#![cfg(target_os = "linux")]

mod common;

use biforgeworks_powerbi::{
    discover_project, ComponentFormat, DiagnosticCode as Code, ProjectComponent, Severity,
};
use common::*;
use std::fs;
use std::path::Path;

fn component(path: &Path, exists: bool, format: ComponentFormat) -> ProjectComponent {
    ProjectComponent {
        path: Some(path.to_string_lossy().into_owned()),
        exists,
        format,
    }
}

#[test]
fn fixture_valid_pbir_tmdl() {
    let root = fixtures_dir().join("valid-pbir-tmdl");
    let summary = discover_project(&root.join("Sales.pbip"));

    assert_eq!(summary.diagnostics, vec![]);
    assert_eq!(summary.project_name, "Sales");
    assert_eq!(
        summary.project_file,
        std::path::absolute(root.join("Sales.pbip"))
            .unwrap()
            .to_string_lossy()
    );
    let abs_root = std::path::absolute(&root).unwrap();
    assert_eq!(summary.project_root, abs_root.to_string_lossy());
    assert_eq!(
        summary.report,
        component(&abs_root.join("Sales.Report"), true, ComponentFormat::Pbir)
    );
    assert_eq!(
        summary.semantic_model,
        component(
            &abs_root.join("Sales.SemanticModel"),
            true,
            ComponentFormat::Tmdl
        )
    );
}

#[test]
fn fixture_valid_legacy() {
    let summary = discover_project(&fixtures_dir().join("valid-legacy/Legacy.pbip"));
    assert_eq!(summary.diagnostics, vec![]);
    assert_eq!(summary.report.format, ComponentFormat::PbirLegacy);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Tmsl);
    assert!(summary.report.exists && summary.semantic_model.exists);
}

#[test]
fn fixture_missing_report() {
    let root = std::path::absolute(fixtures_dir().join("missing-report")).unwrap();
    let summary = discover_project(&root.join("MissingReport.pbip"));
    assert_eq!(
        summary.report,
        component(
            &root.join("MissingReport.Report"),
            false,
            ComponentFormat::Missing
        )
    );
    assert_has(&summary, Code::ReportFolderNotFound);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Missing);
    assert_has(&summary, Code::SemanticModelReferenceMissing);
}

#[test]
fn fixture_missing_model() {
    let root = std::path::absolute(fixtures_dir().join("missing-model")).unwrap();
    let summary = discover_project(&root.join("MissingModel.pbip"));
    assert_eq!(summary.report.format, ComponentFormat::Pbir);
    assert_eq!(
        summary.semantic_model,
        component(
            &root.join("MissingModel.SemanticModel"),
            false,
            ComponentFormat::Missing
        )
    );
    assert_has(&summary, Code::SemanticModelFolderNotFound);
}

#[test]
fn fixture_malformed_pbip() {
    let summary = discover_project(&fixtures_dir().join("malformed-pbip/Malformed.pbip"));
    assert_has(&summary, Code::PbipInvalidJson);
    assert_eq!(summary.report.format, ComponentFormat::Missing);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Missing);
    let diag = summary
        .diagnostics
        .iter()
        .find(|d| d.code == Code::PbipInvalidJson)
        .unwrap();
    assert_eq!(diag.severity, Severity::Error);
    assert_eq!(diag.path.as_deref(), Some(summary.project_file.as_str()));
}

#[test]
fn fixture_unknown_formats() {
    let summary = discover_project(&fixtures_dir().join("unknown-formats/Unknown.pbip"));
    assert!(summary.report.exists && summary.semantic_model.exists);
    assert_eq!(summary.report.format, ComponentFormat::Unknown);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Unknown);
    assert_has(&summary, Code::UnknownReportFormat);
    assert_has(&summary, Code::UnknownModelFormat);
}

#[test]
fn serialized_contract_uses_snake_case_and_documented_strings() {
    let summary = discover_project(&fixtures_dir().join("valid-pbir-tmdl/Sales.pbip"));
    let value = serde_json::to_value(&summary).unwrap();
    let keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for key in [
        "project_file",
        "project_root",
        "project_name",
        "report",
        "semantic_model",
        "diagnostics",
    ] {
        assert!(keys.contains(&key), "{key}");
    }
    assert_eq!(value["report"]["format"], "PBIR");
    assert_eq!(value["report"]["exists"], true);
    assert!(value["report"]["path"].is_string());
    assert_eq!(value["semantic_model"]["format"], "TMDL");

    let legacy = discover_project(&fixtures_dir().join("valid-legacy/Legacy.pbip"));
    let value = serde_json::to_value(&legacy).unwrap();
    assert_eq!(value["report"]["format"], "PBIR_LEGACY");
    assert_eq!(value["semantic_model"]["format"], "TMSL");

    let missing = discover_project(&fixtures_dir().join("missing-report/MissingReport.pbip"));
    let value = serde_json::to_value(&missing).unwrap();
    assert_eq!(value["semantic_model"]["format"], "MISSING");
    assert_eq!(value["semantic_model"]["path"], serde_json::Value::Null);
    let diag = &value["diagnostics"][0];
    assert_eq!(diag["severity"], "error");
    assert_eq!(diag["code"], "REPORT_FOLDER_NOT_FOUND");
    assert!(diag["message"].is_string());
    assert!(diag["path"].is_string());
    assert_eq!(value["diagnostics"][1]["severity"], "warning");

    let unknown = discover_project(&fixtures_dir().join("unknown-formats/Unknown.pbip"));
    assert_eq!(
        serde_json::to_value(&unknown).unwrap()["report"]["format"],
        "UNKNOWN"
    );
}

#[test]
fn cross_format_combinations_are_identified() {
    let tmp = TempDir::new("cross");

    // PBIR report with a TMSL model.
    let a = tmp.path().join("a");
    let pbip = build_pbir_tmdl(&a, "A");
    fs::remove_dir_all(a.join("A.SemanticModel/definition")).unwrap();
    write(&a.join("A.SemanticModel/model.bim"), "{}");
    let summary = discover_project(&pbip);
    assert_eq!(summary.diagnostics, vec![]);
    assert_eq!(summary.report.format, ComponentFormat::Pbir);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Tmsl);

    // PBIR-Legacy report (4.0 permits either) with a TMDL model.
    let b = tmp.path().join("b");
    let pbip = build_pbir_tmdl(&b, "B");
    fs::remove_dir_all(b.join("B.Report/definition")).unwrap();
    write(&b.join("B.Report/report.json"), "{}");
    let summary = discover_project(&pbip);
    assert_eq!(summary.diagnostics, vec![]);
    assert_eq!(summary.report.format, ComponentFormat::PbirLegacy);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Tmdl);
}

#[test]
fn formats_come_from_markers_not_folder_names() {
    let tmp = TempDir::new("names");
    let root = tmp.path();
    write(&root.join("Odd.pbip"), pbip_json("reports/first"));
    write(
        &root.join("reports/first/definition.pbir"),
        pbir_json("4.0", "../../models/x"),
    );
    write(&root.join("reports/first/report.json"), "{}");
    write(&root.join("models/x/definition.pbism"), pbism_json("4.0"));
    write(&root.join("models/x/model.bim"), "{}");

    let summary = discover_project(&root.join("Odd.pbip"));
    assert_eq!(summary.diagnostics, vec![]);
    assert_eq!(summary.report.format, ComponentFormat::PbirLegacy);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Tmsl);
    assert_eq!(
        summary.semantic_model.path.as_deref(),
        Some(root.join("models/x").to_string_lossy().as_ref())
    );

    // A folder named like a PBIR report is not PBIR without the markers.
    let other = tmp.path().join("other");
    let pbip = build_pbir_tmdl(&other, "Named");
    fs::remove_file(other.join("Named.Report/definition/version.json")).unwrap();
    let summary = discover_project(&pbip);
    assert_eq!(summary.report.format, ComponentFormat::Unknown);
    assert_has(&summary, Code::UnknownReportFormat);
}

#[test]
fn relative_references_resolve_with_dot_segments_and_trailing_slashes() {
    let tmp = TempDir::new("relative");
    let root = tmp.path();
    write(&root.join("P.pbip"), pbip_json("./nested/../R/"));
    write(&root.join("R/definition.pbir"), pbir_json("4.0", "./../M"));
    write(&root.join("R/report.json"), "{}");
    write(&root.join("M/definition.pbism"), pbism_json("1.0"));
    write(&root.join("M/model.bim"), "{}");
    let summary = discover_project(&root.join("P.pbip"));
    assert_eq!(summary.diagnostics, vec![]);
    assert_eq!(
        summary.report.path.as_deref(),
        Some(root.join("R").to_string_lossy().as_ref())
    );
    assert_eq!(summary.semantic_model.format, ComponentFormat::Tmsl);
}

#[test]
fn entry_path_problems_are_diagnosed() {
    let tmp = TempDir::new("entry");

    let summary = discover_project(Path::new(""));
    assert_has(&summary, Code::PbipInvalidPath);

    let summary = discover_project(Path::new("/"));
    assert_has(&summary, Code::PbipInvalidPath);

    write(&tmp.path().join("notes.txt"), "x");
    let summary = discover_project(&tmp.path().join("notes.txt"));
    assert_has(&summary, Code::PbipInvalidExtension);

    let summary = discover_project(&tmp.path().join("absent.pbip"));
    assert_has(&summary, Code::PbipNotFound);
    assert_eq!(summary.project_name, "absent");

    let summary = discover_project(&tmp.path().join("no-such-dir/absent.pbip"));
    assert_has(&summary, Code::PbipNotFound);

    fs::create_dir(tmp.path().join("folder.pbip")).unwrap();
    let summary = discover_project(&tmp.path().join("folder.pbip"));
    assert_has(&summary, Code::NotARegularFile);

    // Extension matching is case-insensitive (Windows-authored projects).
    let upper = tmp.path().join("upper");
    let pbip = build_pbir_tmdl(&upper, "Upper");
    let renamed = upper.join("Upper.PBIP");
    fs::rename(&pbip, &renamed).unwrap();
    assert_eq!(discover_project(&renamed).diagnostics, vec![]);
}

#[test]
fn missing_and_ambiguous_references() {
    let tmp = TempDir::new("refs");
    let root = tmp.path();

    let cases: &[(&str, Code)] = &[
        (
            r#"{"version": "1.0", "artifacts": []}"#,
            Code::ReportReferenceMissing,
        ),
        (r#"{"version": "1.0"}"#, Code::ReportReferenceMissing),
        (
            r#"{"version": "1.0", "artifacts": {}}"#,
            Code::ReportReferenceInvalid,
        ),
        (
            r#"{"version": "1.0", "artifacts": [{"report": {}}]}"#,
            Code::ReportReferenceInvalid,
        ),
        (
            r#"{"version": "1.0", "artifacts": [{"report": {"path": ""}}]}"#,
            Code::ReportReferenceInvalid,
        ),
        (
            r#"{"version": "1.0", "artifacts": [{"report": {"path": "A"}}, {"report": {"path": "B"}}]}"#,
            Code::ReportReferenceAmbiguous,
        ),
        (r#"[]"#, Code::PbipInvalidStructure),
        (
            r#"{"version": "1.0", "version": "1.0", "artifacts": []}"#,
            Code::PbipInvalidJson,
        ),
    ];
    for (index, (json, code)) in cases.iter().enumerate() {
        let pbip = root.join(format!("case{index}.pbip"));
        write(&pbip, json);
        let summary = discover_project(&pbip);
        assert_has(&summary, *code);
        assert!(has_error(&summary), "{json}");
        assert_eq!(summary.report.format, ComponentFormat::Missing, "{json}");
    }
}

#[test]
fn semantic_model_reference_shapes() {
    let tmp = TempDir::new("model-refs");
    let cases: &[(&str, Code)] = &[
        (r#"{}"#, Code::SemanticModelReferenceMissing),
        (
            r#"{"byPath": null, "byConnection": null}"#,
            Code::SemanticModelReferenceMissing,
        ),
        (
            r#"{"byPath": {"path": "../M"}, "byConnection": {"connectionString": "x"}}"#,
            Code::SemanticModelReferenceAmbiguous,
        ),
        (
            r#"{"byPath": {"path": 1}}"#,
            Code::SemanticModelReferenceInvalid,
        ),
        (
            r#"{"byPath": {"path": ""}}"#,
            Code::SemanticModelReferenceInvalid,
        ),
    ];
    for (index, (reference, code)) in cases.iter().enumerate() {
        let root = tmp.path().join(format!("case{index}"));
        let pbip = build_pbir_tmdl(&root, "C");
        write(
            &root.join("C.Report/definition.pbir"),
            format!(
                r#"{{"$schema": "{PBIR_SCHEMA_V2}", "version": "4.0", "datasetReference": {reference}}}"#
            ),
        );
        let summary = discover_project(&pbip);
        assert_eq!(summary.report.format, ComponentFormat::Pbir, "{reference}");
        assert_has(&summary, *code);
        assert_eq!(summary.semantic_model.format, ComponentFormat::Missing);
    }
}

#[test]
fn remote_model_is_diagnosed_without_exposing_connection_details() {
    let tmp = TempDir::new("remote");
    let root = tmp.path();
    let pbip = build_pbir_tmdl(root, "Remote");
    let secret =
        "Data Source=powerbi://api.powerbi.com/v1.0/myorg/Secret-Workspace;Password=hunter2";
    write(
        &root.join("Remote.Report/definition.pbir"),
        serde_json::json!({
            "$schema": PBIR_SCHEMA_V2,
            "version": "4.0",
            "datasetReference": { "byPath": null, "byConnection": { "connectionString": secret } },
        })
        .to_string(),
    );
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::SemanticModelRemoteUnsupported);
    assert_eq!(
        summary.semantic_model,
        ProjectComponent {
            path: None,
            exists: false,
            format: ComponentFormat::Missing,
        }
    );
    let out = serialized(&summary);
    for fragment in ["hunter2", "Secret-Workspace", "powerbi://", "Data Source"] {
        assert!(!out.contains(fragment), "leaked {fragment}: {out}");
    }
}

#[test]
fn definition_files_missing_or_malformed() {
    let tmp = TempDir::new("defs");

    let root = tmp.path().join("no-pbir");
    let pbip = build_pbir_tmdl(&root, "N");
    fs::remove_file(root.join("N.Report/definition.pbir")).unwrap();
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::ReportDefinitionMissing);
    assert!(summary.report.exists);
    assert_eq!(summary.report.format, ComponentFormat::Unknown);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Missing);

    let root = tmp.path().join("no-pbism");
    let pbip = build_pbir_tmdl(&root, "N");
    fs::remove_file(root.join("N.SemanticModel/definition.pbism")).unwrap();
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::SemanticModelDefinitionMissing);
    assert!(summary.semantic_model.exists);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Unknown);

    let root = tmp.path().join("bad-pbir");
    let pbip = build_pbir_tmdl(&root, "N");
    write(&root.join("N.Report/definition.pbir"), "{ not json");
    assert_has(&discover_project(&pbip), Code::ReportDefinitionInvalidJson);

    let root = tmp.path().join("array-pbir");
    let pbip = build_pbir_tmdl(&root, "N");
    write(&root.join("N.Report/definition.pbir"), "[]");
    assert_has(
        &discover_project(&pbip),
        Code::ReportDefinitionInvalidStructure,
    );

    let root = tmp.path().join("bad-pbism");
    let pbip = build_pbir_tmdl(&root, "N");
    write(
        &root.join("N.SemanticModel/definition.pbism"),
        "{\"version\": ",
    );
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::SemanticModelDefinitionInvalidJson);
    assert_eq!(summary.semantic_model.format, ComponentFormat::Unknown);

    let root = tmp.path().join("bad-version-json");
    let pbip = build_pbir_tmdl(&root, "N");
    write(&root.join("N.Report/definition/version.json"), "nope");
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::ReportVersionMetadataInvalid);
    assert_eq!(summary.report.format, ComponentFormat::Unknown);
}

#[test]
fn unsupported_or_future_versions_never_validate() {
    let tmp = TempDir::new("versions");
    let mut n = 0;
    let mut project = |edit: &dyn Fn(&Path)| {
        n += 1;
        let root = tmp.path().join(format!("p{n}"));
        let pbip = build_pbir_tmdl(&root, "V");
        edit(&root);
        discover_project(&pbip)
    };

    let s = project(&|r| {
        write(
            &r.join("V.Report/definition.pbir"),
            pbir_json("5.0", "../V.SemanticModel"),
        )
    });
    assert_eq!(s.report.format, ComponentFormat::Unknown);
    assert_has(&s, Code::ReportDefinitionVersionUnsupported);
    // The model reference is still followed.
    assert_eq!(s.semantic_model.format, ComponentFormat::Tmdl);

    let s = project(&|r| {
        write(
            &r.join("V.Report/definition.pbir"),
            pbir_json("2.0", "../V.SemanticModel"),
        )
    });
    assert_eq!(s.report.format, ComponentFormat::Unknown);

    let s = project(&|r| {
        write(
            &r.join("V.Report/definition.pbir"),
            pbir_json("4.0", "../V.SemanticModel").replace("/2.0.0/", "/3.0.0/"),
        )
    });
    assert_eq!(s.report.format, ComponentFormat::Unknown);
    assert_has(&s, Code::ReportDefinitionSchemaUnsupported);

    let s = project(&|r| {
        write(
            &r.join("V.Report/definition.pbir"),
            r#"{"version": "4.0", "datasetReference": {"byPath": {"path": "../V.SemanticModel"}}}"#,
        )
    });
    assert_eq!(s.report.format, ComponentFormat::Pbir);
    assert_has(&s, Code::MetadataSchemaMissing);

    let s = project(&|r| {
        write(
            &r.join("V.SemanticModel/definition.pbism"),
            pbism_json("3.0"),
        )
    });
    assert_eq!(s.semantic_model.format, ComponentFormat::Unknown);
    assert_has(&s, Code::SemanticModelDefinitionVersionUnsupported);

    let s = project(&|r| {
        write(
            &r.join("V.SemanticModel/definition.pbism"),
            pbism_json("4.0").replace("/1.0.0/", "/2.0.0/"),
        )
    });
    assert_eq!(s.semantic_model.format, ComponentFormat::Unknown);
    assert_has(&s, Code::SemanticModelDefinitionSchemaUnsupported);

    let s = project(&|r| {
        write(
            &r.join("V.Report/definition/version.json"),
            version_json("3.0.0"),
        )
    });
    assert_eq!(s.report.format, ComponentFormat::Unknown);
    assert_has(&s, Code::ReportVersionMetadataUnsupported);

    let s = project(&|r| {
        write(
            &r.join("V.Report/definition/version.json"),
            version_json("2.0.0").replace("/1.0.0/", "/2.0.0/"),
        )
    });
    assert_eq!(s.report.format, ComponentFormat::Unknown);
    assert_has(&s, Code::ReportVersionMetadataUnsupported);

    // A future .pbip envelope is flagged but the pointer is still followed.
    let s = project(&|r| {
        write(
            &r.join("V.pbip"),
            pbip_json("V.Report")
                .replace("/1.0.0/", "/2.0.0/")
                .replace("\"1.0\"", "\"2.0\""),
        )
    });
    assert_has(&s, Code::PbipSchemaUnsupported);
    assert_has(&s, Code::PbipVersionUnsupported);
    assert_eq!(s.report.format, ComponentFormat::Pbir);

    // 4.x minors are documented as "4.0 or above" and accepted.
    let s = project(&|r| {
        write(
            &r.join("V.SemanticModel/definition.pbism"),
            pbism_json("4.2"),
        )
    });
    assert_eq!(s.diagnostics, vec![]);
}

#[test]
fn conflicting_markers_are_ambiguous() {
    let tmp = TempDir::new("ambiguous");
    let mut n = 0;
    let mut project = |edit: &dyn Fn(&Path)| {
        n += 1;
        let root = tmp.path().join(format!("p{n}"));
        let pbip = build_pbir_tmdl(&root, "X");
        edit(&root);
        discover_project(&pbip)
    };

    let s = project(&|r| write(&r.join("X.Report/report.json"), "{}"));
    assert_eq!(s.report.format, ComponentFormat::Unknown);
    assert_has(&s, Code::ReportFormatAmbiguous);

    let s = project(&|r| write(&r.join("X.SemanticModel/model.bim"), "{}"));
    assert_eq!(s.semantic_model.format, ComponentFormat::Unknown);
    assert_has(&s, Code::ModelFormatAmbiguous);

    let s = project(&|r| {
        write(
            &r.join("X.Report/definition.pbir"),
            pbir_json("1.0", "../X.SemanticModel"),
        )
    });
    assert_eq!(s.report.format, ComponentFormat::Unknown);
    assert_has(&s, Code::ReportFormatVersionMismatch);

    let s = project(&|r| {
        write(
            &r.join("X.SemanticModel/definition.pbism"),
            pbism_json("1.0"),
        )
    });
    assert_eq!(s.semantic_model.format, ComponentFormat::Unknown);
    assert_has(&s, Code::ModelFormatVersionMismatch);

    let s = project(&|r| fs::remove_file(r.join("X.SemanticModel/definition/model.tmdl")).unwrap());
    assert_eq!(s.semantic_model.format, ComponentFormat::Unknown);
    assert_has(&s, Code::UnknownModelFormat);

    let s = project(&|r| fs::remove_file(r.join("X.Report/definition/report.json")).unwrap());
    assert_eq!(s.report.format, ComponentFormat::Unknown);
    assert_has(&s, Code::UnknownReportFormat);
}

#[test]
fn unsupported_artifacts_are_flagged() {
    let tmp = TempDir::new("artifacts");
    let pbip = build_pbir_tmdl(tmp.path(), "A");
    write(
        &pbip,
        serde_json::json!({
            "$schema": PBIP_SCHEMA,
            "version": "1.0",
            "artifacts": [{ "dashboard": {} }, { "report": { "path": "A.Report" } }],
        })
        .to_string(),
    );
    let summary = discover_project(&pbip);
    assert_has(&summary, Code::PbipArtifactUnsupported);
    assert_eq!(summary.report.format, ComponentFormat::Pbir);
}
