use biforgeworks_powerbi::tmdl::{
    self, diagnostics::TmdlCode as Code, model::*, parser::parse_document,
};
use biforgeworks_powerbi::Severity;

fn inspect(text: &str) -> SemanticInspection {
    tmdl::inspect_sources("test-project", vec![("model.tmdl".into(), text.into())])
}
fn valid(text: &str) -> SemanticModel {
    let result = inspect(text);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "{:?}",
        result.diagnostics
    );
    result.model.unwrap()
}
fn has(text: &str, code: Code) {
    assert!(inspect(text).diagnostics.iter().any(|d| d.code == code));
}

#[test]
fn metadata_and_annotations() {
    let m = valid("database Sales\n\tcompatibilityLevel: 1702\n/// Documentation\nmodel Model\n\tculture: en-US\n\tsourceQueryCulture: en-US\n\tannotation Note = keep me\n");
    assert_eq!(m.database.unwrap().name, "Sales");
    assert_eq!(m.metadata.description.as_deref(), Some("Documentation"));
    assert_eq!(m.metadata.annotations[0].value, "keep me");
    assert_eq!(m.metadata.properties[0].value, "en-US");
}
#[test]
fn quoted_identifiers_and_property_values() {
    let m = valid("model Model\ntable 'A.B''s'\n\tcolumn 'Net: Price'\n\t\tdataType: decimal\n\t\tsourceColumn: \" Net \"\"Price\"\" \"\n\t\tisHidden\n");
    assert_eq!(m.tables[0].metadata.name, "A.B's");
    let c = &m.tables[0].columns[0];
    assert_eq!(c.source_column.as_deref(), Some(" Net \"Price\" "));
    assert!(c.metadata.is_hidden);
}
#[test]
fn calculated_and_calculated_table_columns() {
    let m = valid("model Model\ntable T\n\tcolumn X = 1 + 2\n\t\tdataType: int64\n\tcolumn Y\n\t\tsourceColumn: [Value]\n\tpartition P = calculated\n\t\tsource = {1,2}\n");
    assert!(matches!(
        m.tables[0].columns[0].kind,
        ColumnKind::Calculated
    ));
    assert!(matches!(
        m.tables[0].columns[1].kind,
        ColumnKind::CalculatedTable
    ));
}
#[test]
fn multiline_dax_keeps_boundaries_blank_lines_and_source() {
    let text = "model Model\ntable T\n\tmeasure M =\n\t\t\tVAR x = 1\n\n\t\t\tRETURN x\n\t\tformatString: 0.0%\n\tmeasure N = 2\n";
    let result = inspect(text);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let measures = &result.model.unwrap().tables[0].measures;
    assert_eq!(measures.len(), 2);
    let e = measures[0].expression.as_ref().unwrap();
    assert_eq!(e.raw, "\t\t\tVAR x = 1\n\n\t\t\tRETURN x\n");
    assert_eq!(e.display, "VAR x = 1\n\nRETURN x\n");
    assert_eq!(&text[e.span.start..e.span.end], e.raw);
    assert_eq!(measures[0].format_string.as_deref(), Some("0.0%"));
}
#[test]
fn multiline_m_is_opaque() {
    let m = valid("model Model\ntable T\n\tpartition P = m\n\t\tmode: import\n\t\tsource =\n\t\t\tlet\n\t\t\t    x = #table({\"table\"}, {})\n\t\t\tin x\n");
    let p = &m.tables[0].partitions[0];
    assert_eq!(p.source_kind.as_deref(), Some("m"));
    assert!(p.expression.as_ref().unwrap().raw.contains("#table"));
}
#[test]
fn fences_ignore_structural_looking_expression_lines() {
    let m = valid("model Model\ntable T\n\tmeasure M = ```\ntable ThisIsDaxText\n\n  trailing  \n\t\t\t```\n\t\tformatString: 0\n");
    let e = m.tables[0].measures[0].expression.as_ref().unwrap();
    assert!(e.fenced);
    assert_eq!(e.raw, "table ThisIsDaxText\n\n  trailing  \n");
}
#[test]
fn utf8_bom_crlf_spans_and_no_final_newline() {
    let text = "\u{feff}model Model\r\ntable Café\r\n\tmeasure '€' = 1";
    let result = inspect(text);
    assert_eq!(result.documents[0].text, text);
    let m = result.model.unwrap();
    let e = m.tables[0].measures[0].expression.as_ref().unwrap();
    assert_eq!(&text[e.span.start..e.span.end], "1");
    assert_eq!(e.span.line, 3);
    assert_eq!(m.metadata.sources[0].start, 3);
}
#[test]
fn hierarchy_order_and_reference_resolution() {
    let m = valid("model Model\ntable T\n\tcolumn C\n\thierarchy H\n\t\tlevel second\n\t\t\tordinal: 1\n\t\t\tcolumn: C\n\t\tlevel first\n\t\t\tordinal: 0\n\t\t\tcolumn: C\n");
    assert_eq!(m.tables[0].hierarchies[0].levels[0].metadata.name, "first");
}
#[test]
fn relationship_quoted_endpoints_and_behavior() {
    let m = valid("model Model\ntable 'T.1'\n\tcolumn 'C.1'\ntable U\n\tcolumn K\nrelationship R\n\tfromColumn: 'T.1'.'C.1'\n\ttoColumn: U.K\n\tisActive: false\n\tcrossFilteringBehavior: bothDirections\n");
    let r = &m.relationships[0];
    assert!(r.from.as_ref().unwrap().resolved);
    assert!(!r.is_active);
    assert_eq!(r.cross_filter, "bothDirections");
}
#[test]
fn roles_and_perspectives() {
    let m = valid("model Model\ntable T\n\tcolumn C\n\tmeasure M = 1\nrole Reader\n\tmodelPermission: read\n\ttablePermission T = [C] > 0\nperspective P\n\tperspectiveTable T\n\t\tperspectiveColumn C\n\t\tperspectiveMeasure M\n");
    assert!(m.roles[0].filters[0].resolved);
    assert_eq!(
        m.roles[0].filters[0].expression.as_ref().unwrap().raw,
        "[C] > 0"
    );
    assert!(m.perspectives[0].tables[0].resolved);
}
#[test]
fn culture_translation_and_linguistic_content() {
    let m = valid("model Model\nculture fr-FR\n\ttranslations\n\t\tmodel Model\n\t\t\ttable T\n\t\t\t\tcaption: Ventes\n\tlinguisticMetadata = ```\n{\"Language\":\"fr-FR\"}\n\t\t```\n");
    assert_eq!(m.cultures[0].translations[0].properties[0].value, "Ventes");
    assert!(m.cultures[0].linguistic_metadata.is_some());
}
#[test]
fn named_expressions_and_functions() {
    let m = valid("model Model\nexpression Parameter =\n\t\tlet x = 1 in x\n\tkind: m\nfunction Twice = (x: NUMERIC) => x * 2\n");
    assert_eq!(m.expressions[0].kind.as_deref(), Some("m"));
    assert!(m.functions[0]
        .expression
        .as_ref()
        .unwrap()
        .raw
        .contains("=>"));
}
#[test]
fn partial_tables_merge_and_refs_order() {
    let r = tmdl::inspect_sources(
        "test",
        vec![
            (
                "a".into(),
                "model Model\nref table B\nref table A\ntable A\n\tcolumn X\ntable B\n".into(),
            ),
            ("b".into(), "ref table A\n\tmeasure M = 1\n".into()),
        ],
    );
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let m = r.model.unwrap();
    assert_eq!(m.tables[0].metadata.name, "B");
    assert_eq!(m.tables[1].columns.len(), 1);
    assert_eq!(m.tables[1].measures.len(), 1);
    assert_eq!(m.tables[1].metadata.sources.len(), 2);
}
#[test]
fn unknown_syntax_warns_and_remains_in_source() {
    let text = "model Model\n\tfutureFlag: preserve\n\tfutureObject X\n\t\tfuture: 42\n";
    let r = inspect(text);
    assert!(matches!(r.status, InspectionStatus::Ready));
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == Code::TmdlUnknownProperty));
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == Code::TmdlUnsupportedObject));
    assert_eq!(r.documents[0].text, text);
}
#[test]
fn duplicate_objects_and_properties() {
    has(
        "model Model\ntable T\n\tmeasure M = 1\n\tmeasure M = 2\n",
        Code::TmdlDuplicateObject,
    );
    has(
        "model Model\ntable T\n\tlineageTag: x\ntable T\n\tlineageTag: y\n",
        Code::TmdlDuplicateObject,
    );
}
#[test]
fn broken_references_diagnose_without_crashing() {
    has(
        "model Model\nrelationship R\n\tfromColumn: Missing.C\n\ttoColumn: Other.C\n",
        Code::TmdlInvalidRelationshipReference,
    );
    has(
        "model Model\ntable T\n\thierarchy H\n\t\tlevel L\n\t\t\tcolumn: Missing\n",
        Code::TmdlInvalidHierarchyReference,
    );
    has(
        "model Model\nperspective P\n\tperspectiveTable Missing\n",
        Code::TmdlUnresolvedReference,
    );
}
#[test]
fn malformed_syntax_diagnoses_with_source() {
    for text in [
        "model Model\n    culture: en-US\n   table T\n",
        "model Model\ntable 'unclosed\n",
        "model Model\ntable T\n\tmeasure M = ```\nx",
        "model Model\ntable T\n\tmeasure M =\n\t\tformatString: 0\n",
        "model Model\n\tculture: \"bad\n",
    ] {
        let r = inspect(text);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == Code::TmdlSyntaxError && d.source.is_some()),
            "{text}"
        );
    }
}
#[test]
fn four_space_indentation_and_case_insensitive_properties() {
    let m = valid("model Model\ntable T\n    column C\n        DATATYPE: int64\n");
    assert_eq!(m.tables[0].columns[0].data_type.as_deref(), Some("int64"));
}
#[test]
fn identities_do_not_depend_on_array_index_or_source_line() {
    let a = valid("model Model\ntable T\n\tmeasure M = 1\n\t\tlineageTag: fixed\n");
    let b = valid(
        "model Model\n\ntable Other\ntable T\n\tmeasure Renamed = 2\n\t\tlineageTag: fixed\n",
    );
    assert_eq!(
        a.tables[0].measures[0].metadata.id,
        b.tables[1].measures[0].metadata.id
    );
}
#[test]
fn hostile_depth_and_size_are_bounded() {
    let text = (0..200)
        .map(|i| format!("{}future X\n", "\t".repeat(i)))
        .collect::<String>();
    assert!(!inspect(&text).diagnostics.is_empty());
    let (_, diagnostics) = parse_document("huge", "x".repeat(tmdl::parser::MAX_SOURCE_BYTES + 1));
    assert_eq!(diagnostics[0].code, Code::TmdlLimitExceeded);
}
#[test]
fn moderate_model_performance() {
    let text = include_str!("../../../fixtures/powerbi/tmdl-performance/model.tmdl");
    let started = std::time::Instant::now();
    let result = inspect(text);
    let elapsed = started.elapsed();
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let m = result.model.unwrap();
    assert_eq!(m.tables.len(), 50);
    assert_eq!(m.tables.iter().map(|t| t.columns.len()).sum::<usize>(), 500);
    assert_eq!(
        m.tables.iter().map(|t| t.measures.len()).sum::<usize>(),
        250
    );
    assert_eq!(m.relationships.len(), 100);
    println!("WP03 performance: 50 tables / 500 columns / 250 measures / 100 relationships; {} source bytes; {elapsed:?}", text.len());
}

#[test]
fn inferred_space_units_match_tab_documents() {
    let text = "model Model\n\tculture: en-US\ntable T\n\tmeasure M =\n\t\t\t1 + 2\n\t\tformatString: 0\n\tcolumn C\n\t\tdataType: int64\n";
    for width in [1, 2, 3, 4, 8] {
        let model = valid(&text.replace('\t', &" ".repeat(width)));
        assert_eq!(model.tables[0].columns.len(), 1);
        assert_eq!(
            model.tables[0].measures[0]
                .expression
                .as_ref()
                .unwrap()
                .display,
            "1 + 2\n"
        );
    }
}
#[test]
fn fenced_closer_uses_literal_prefix_and_accepts_trailing_space() {
    let m = valid("model Model\ntable T\n\tmeasure M = ```\n   1 + 2  \n   ```  \n\tcolumn C\n");
    assert_eq!(m.tables[0].columns.len(), 1);
    assert_eq!(
        m.tables[0].measures[0].expression.as_ref().unwrap().display,
        "1 + 2  \n"
    );
}
#[test]
fn invalid_document_is_not_projected_as_reliable_content() {
    let r = tmdl::inspect_sources(
        "test",
        vec![
            ("model".into(), "model Model\n".into()),
            (
                "bad-table".into(),
                "table T\n\tmeasure M = ```\n\tcolumn C\n".into(),
            ),
        ],
    );
    assert!(matches!(r.status, InspectionStatus::Partial));
    assert!(r.model.unwrap().tables.is_empty());
    assert!(!r.documents[1].is_valid);
    assert!(r.documents[1].text.contains("column C"));
}
#[test]
fn dotted_functions_and_uppercase_keywords() {
    let m = valid(
        "MODEL Model\nFUNCTION Contoso.Twice = (x) => x * 2\nFUNCTION 'Contoso.Other' = (x) => x\n",
    );
    assert_eq!(m.functions.len(), 2);
    assert_eq!(m.functions[0].metadata.name, "Contoso.Twice");
}
#[test]
fn role_members_and_column_permissions_are_inspectable() {
    let m = valid("model Model\ntable T\n\tcolumn C\nrole Reader\n\ttablePermission T = 1 = 1\n\t\tcolumnPermission C = none\n\tmember 'someone@example.invalid'\n\tmember 'group@example.invalid' = group\n\t\tidentityProvider = example\n\tmember domain\\user = activeDirectory\n");
    assert_eq!(m.roles[0].members.len(), 3);
    assert_eq!(
        m.roles[0].members[1].identity_provider.as_deref(),
        Some("example")
    );
    assert_eq!(
        m.roles[0].filters[0].column_permissions[0]
            .permission
            .as_deref(),
        Some("none")
    );
}
#[test]
fn culture_has_no_spurious_unsupported_warnings() {
    let r = inspect("model Model\nculture fr-FR\n\ttranslations\n\t\tmodel Model\n\t\t\tcaption: Modèle\n\tlinguisticMetadata = {}\n");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
}
#[test]
fn bom_does_not_shift_visible_column() {
    let (d, _) = parse_document("bom", "\u{feff}model Model".into());
    assert_eq!(d.nodes[0].span.column, 1);
}
#[test]
fn general_comments_and_missing_names_are_not_silently_valid() {
    has("model Model\n// not a description\n", Code::TmdlSyntaxError);
    has("model Model\ntable\n", Code::TmdlSyntaxError);
    has("model Model\ntable ''\n", Code::TmdlSyntaxError);
}

#[test]
fn anonymous_database_and_invalid_parent_scopes() {
    let m = valid("database\n\tcompatibilityLevel: 1702\nmodel Model\n");
    assert!(m.database.is_some());
    has("model Model\n\tcolumn C\n", Code::TmdlSyntaxError);
    has(
        "model Model\ntable T\n\tmeasure M = 1\n\t\tcolumn Wrong\n",
        Code::TmdlSyntaxError,
    );
    has("model Model\nmodel Other\n", Code::TmdlDuplicateObject);
}

#[test]
fn partial_measure_properties_retain_all_source_locations() {
    let r = tmdl::inspect_sources(
        "test",
        vec![
            (
                "a.tmdl".into(),
                "model Model\ntable T\n\tmeasure M = 1\n".into(),
            ),
            (
                "b.tmdl".into(),
                "table T\n\tmeasure M\n\t\tformatString: 0.0%\n".into(),
            ),
        ],
    );
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let m = &r.model.unwrap().tables[0].measures[0];
    assert_eq!(m.metadata.sources.len(), 2);
    assert_eq!(m.format_string.as_deref(), Some("0.0%"));
    assert_eq!(m.expression.as_ref().unwrap().raw, "1");
}
#[test]
fn syntax_spans_are_valid_slices_with_disjoint_siblings() {
    fn check(text: &str, nodes: &[tmdl::syntax::SyntaxNode]) {
        let mut end = 0;
        for n in nodes {
            assert!(n.span.start >= end);
            assert!(text.get(n.span.start..n.span.end).is_some());
            assert!(n.header.start >= n.span.start && n.header.end <= n.span.end);
            if let Some(e) = &n.expression {
                assert_eq!(&text[e.span.start..e.span.end], e.raw);
            }
            check(text, &n.children);
            end = n.span.end;
        }
    }
    let text = "model Model\n\ntable T\n\tmeasure A =\n\t\t\t1\n\n\n\t/// A description\n\tmeasure B = 2\n\n";
    let r = inspect(text);
    check(text, &r.documents[0].nodes);
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::path::PathBuf;
    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/powerbi")
            .join(name)
            .join("Sales.pbip")
    }
    #[test]
    fn native_reader_comprehensive_fixture() {
        let r = tmdl::inspect_project(&fixture("tmdl-star-schema"));
        assert!(
            !r.diagnostics.iter().any(|d| d.severity == Severity::Error),
            "{:?}",
            r.diagnostics
        );
        let m = r.model.unwrap();
        assert_eq!(m.tables.len(), 2);
        assert_eq!(m.tables[0].measures.len(), 3);
        assert_eq!(m.relationships.len(), 1);
        assert_eq!(m.roles.len(), 1);
        assert_eq!(m.perspectives.len(), 1);
        assert_eq!(m.cultures.len(), 1);
        assert_eq!(m.expressions.len(), 1);
        assert_eq!(m.functions.len(), 1);
        println!("WP03 demonstration: {} tables / {} columns / {} measures / {} relationships / {} roles / {} perspectives", m.tables.len(), m.tables.iter().map(|t| t.columns.len()).sum::<usize>(), m.tables.iter().map(|t| t.measures.len()).sum::<usize>(), m.relationships.len(), m.roles.len(), m.perspectives.len());
        println!(
            "Measure: {}\n{}\nFormat: {:?}",
            m.tables[0].measures[2].metadata.name,
            m.tables[0].measures[2].expression.as_ref().unwrap().display,
            m.tables[0].measures[2].format_string
        );
    }
    #[test]
    fn tmsl_is_explicitly_unsupported() {
        let r = tmdl::inspect_project(&fixture("valid-legacy").with_file_name("Legacy.pbip"));
        assert!(matches!(r.status, InspectionStatus::Unsupported));
        assert!(r.model.is_none());
    }
}

#[test]
fn excessive_lines_are_rejected_before_tokenization() {
    let result =
        tmdl::inspect_sources("bounded", vec![("model.tmdl".into(), "\n".repeat(200_001))]);
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == Code::TmdlLimitExceeded));
}

#[test]
fn permission_identity_is_scoped_to_table_and_perspective_resolution_is_complete() {
    let model = valid("model Model\ntable A\n\tcolumn Key\ntable B\n\tcolumn Key\nrole Reader\n\ttablePermission A\n\t\tcolumnPermission Key = none\n\ttablePermission B\n\t\tcolumnPermission Key = none\nperspective View\n\tperspectiveTable A\n\t\tperspectiveColumn Missing\n");
    let filters = &model.roles[0].filters;
    assert_ne!(
        filters[0].column_permissions[0].metadata.id,
        filters[1].column_permissions[0].metadata.id
    );
    assert!(!model.perspectives[0].tables[0].resolved);
}
