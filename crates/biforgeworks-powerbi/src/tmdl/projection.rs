use super::{
    diagnostics::{TmdlCode as Code, TmdlDiagnostic},
    lexer,
    model::*,
    syntax::*,
};
use crate::Severity;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

fn property(node: &SyntaxNode, name: &str) -> Option<String> {
    node.children
        .iter()
        .find(|n| n.keyword.eq_ignore_ascii_case(name) && n.name.is_none())
        .map(|n| {
            n.value
                .clone()
                .or_else(|| n.expression.as_ref().map(|e| e.display.trim().to_owned()))
                .unwrap_or_else(|| "true".into())
        })
}
fn ref_property(node: &SyntaxNode, name: &str) -> Option<String> {
    property(node, name).map(|v| {
        lexer::reference(&v)
            .ok()
            .filter(|p| p.len() == 1)
            .map_or(v, |p| p[0].clone())
    })
}
fn child_expression(node: &SyntaxNode, name: &str) -> Option<Expression> {
    node.children
        .iter()
        .find(|n| n.keyword.eq_ignore_ascii_case(name))
        .and_then(|n| n.expression.clone())
}
fn expression(node: &SyntaxNode) -> Option<Expression> {
    node.expression
        .clone()
        .or_else(|| child_expression(node, "expression"))
}
fn children<'a>(node: &'a SyntaxNode, name: &'a str) -> impl Iterator<Item = &'a SyntaxNode> {
    node.children
        .iter()
        .filter(move |n| n.keyword.eq_ignore_ascii_case(name) && n.kind != SyntaxKind::Reference)
}
fn diag(
    diags: &mut Vec<TmdlDiagnostic>,
    code: Code,
    message: impl Into<String>,
    node: &SyntaxNode,
) {
    let severity = if matches!(
        code,
        Code::TmdlUnknownProperty | Code::TmdlUnsupportedObject
    ) {
        Severity::Warning
    } else {
        Severity::Error
    };
    diags.push(TmdlDiagnostic::new(
        severity,
        code,
        message,
        Some(node.header.clone()),
    ));
}
fn id(scope: &str, kind: &str, name: &str, lineage: Option<&str>) -> String {
    let mut hash = Sha256::new();
    for part in [scope, kind, lineage.unwrap_or(name)] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    format!("tmdl:{:x}", hash.finalize())
}

const KNOWN_PROPERTIES: &[&str] = &[
    "culture",
    "sourcequeryculture",
    "description",
    "lineagetag",
    "sourcelineagetag",
    "ishidden",
    "datatype",
    "sourcecolumn",
    "formatstring",
    "summarizeby",
    "datacategory",
    "sortbycolumn",
    "displayfolder",
    "isnullable",
    "iskey",
    "isunique",
    "isavailableinmdx",
    "keepuniquerows",
    "encodinghint",
    "expression",
    "mode",
    "source",
    "query",
    "kind",
    "querygroup",
    "ordinal",
    "column",
    "table",
    "measure",
    "hierarchy",
    "fromcolumn",
    "tocolumn",
    "fromcardinality",
    "tocardinality",
    "crossfilteringbehavior",
    "isactive",
    "securityfilteringbehavior",
    "joinondatebehavior",
    "relyonreferentialintegrity",
    "modelpermission",
    "metadatapermission",
    "filterexpression",
    "compatibilitylevel",
    "id",
    "defaultpowerbidatasourceversion",
    "discourageimplicitmeasures",
    "defaultmode",
    "datasourceversion",
    "defaultdatasourceversion",
    "caption",
    "translatedcaption",
    "translateddescription",
    "translateddisplayfolder",
    "contenttype",
    "state",
    "isprivate",
    "isparameterquery",
    "isparameterqueryrequired",
    "valuetype",
    "format",
    "linguisticmetadata",
    "identityprovider",
    "compatibilitymode",
    "language",
];
const KNOWN_OBJECTS: &[&str] = &[
    "database",
    "model",
    "table",
    "column",
    "measure",
    "hierarchy",
    "level",
    "partition",
    "relationship",
    "role",
    "tablepermission",
    "columnpermission",
    "perspective",
    "perspectivetable",
    "perspectivecolumn",
    "perspectivemeasure",
    "perspectivehierarchy",
    "culture",
    "translation",
    "translations",
    "member",
    "linguisticmetadata",
    "expression",
    "function",
    "annotation",
];

fn metadata(node: &SyntaxNode, scope: &str, diags: &mut Vec<TmdlDiagnostic>) -> ObjectMetadata {
    let name = node.name.clone().unwrap_or_else(|| node.keyword.clone());
    let lineage = property(node, "lineageTag");
    let mut properties = Vec::new();
    let mut annotations = Vec::new();
    for child in &node.children {
        let key = child.keyword.to_ascii_lowercase();
        if key == "annotation" {
            annotations.push(Property {
                name: child.name.clone().unwrap_or_default(),
                value: child
                    .expression
                    .as_ref()
                    .map(|e| e.display.clone())
                    .or(child.value.clone())
                    .unwrap_or_default(),
                source: child.span.clone(),
            });
        } else if child.kind == SyntaxKind::Property
            || child.name.is_none() && child.children.is_empty()
        {
            properties.push(Property {
                name: child.keyword.clone(),
                value: child
                    .value
                    .clone()
                    .or_else(|| child.expression.as_ref().map(|e| e.display.clone()))
                    .unwrap_or_else(|| "true".into()),
                source: child.span.clone(),
            });
            if !KNOWN_PROPERTIES.contains(&key.as_str()) {
                diag(
                    diags,
                    Code::TmdlUnknownProperty,
                    "Unsupported property retained in source",
                    child,
                );
            }
        } else if child.kind != SyntaxKind::Reference && !KNOWN_OBJECTS.contains(&key.as_str()) {
            diag(
                diags,
                Code::TmdlUnsupportedObject,
                "Unsupported object retained in source",
                child,
            );
        }
    }
    ObjectMetadata {
        id: id(
            scope,
            &node.keyword.to_ascii_lowercase(),
            &name,
            lineage.as_deref(),
        ),
        name,
        description: node
            .description
            .clone()
            .or_else(|| property(node, "description")),
        lineage_tag: lineage,
        is_hidden: property(node, "isHidden").is_some_and(|v| v.eq_ignore_ascii_case("true")),
        properties,
        annotations,
        sources: node.declarations.clone(),
    }
}

fn collect_roots(nodes: &[SyntaxNode], roots: &mut Vec<SyntaxNode>) {
    for node in nodes {
        if matches!(
            node.keyword.to_ascii_lowercase().as_str(),
            "database" | "model"
        ) {
            let mut shell = node.clone();
            shell.children.retain(|n| {
                !matches!(
                    n.keyword.to_ascii_lowercase().as_str(),
                    "model"
                        | "table"
                        | "relationship"
                        | "role"
                        | "perspective"
                        | "culture"
                        | "expression"
                        | "function"
                ) || n.kind == SyntaxKind::Property
            });
            roots.push(shell);
            let nested: Vec<_> = node
                .children
                .iter()
                .filter(|n| {
                    matches!(
                        n.keyword.to_ascii_lowercase().as_str(),
                        "model"
                            | "table"
                            | "relationship"
                            | "role"
                            | "perspective"
                            | "culture"
                            | "expression"
                            | "function"
                    ) && n.kind != SyntaxKind::Property
                })
                .cloned()
                .collect();
            collect_roots(&nested, roots);
        } else {
            roots.push(node.clone());
        }
    }
}

fn validate_duplicates(node: &SyntaxNode, diags: &mut Vec<TmdlDiagnostic>) {
    let mut keys = BTreeSet::new();
    for child in &node.children {
        if child.kind != SyntaxKind::Reference {
            let key = (child.keyword.to_ascii_lowercase(), child.name.clone());
            if !keys.insert(key) {
                diag(
                    diags,
                    Code::TmdlDuplicateObject,
                    "Object or property is declared more than once",
                    child,
                );
            }
        }
        validate_duplicates(child, diags);
    }
}

/// Partial declarations contribute properties, but cannot redefine a property.
/// Original document syntax is never mutated; this operates on projection copies.
fn merge_partial(
    existing: &mut SyntaxNode,
    mut incoming: SyntaxNode,
    diags: &mut Vec<TmdlDiagnostic>,
) {
    if existing.expression.is_some() && incoming.expression.is_some()
        || existing.value.is_some() && incoming.value.is_some()
        || existing.description.is_some() && incoming.description.is_some()
    {
        diag(
            diags,
            Code::TmdlDuplicateObject,
            "Partial declaration repeats a property; see both declaration locations",
            &incoming,
        );
        diag(
            diags,
            Code::TmdlDuplicateObject,
            "Earlier declaration of the repeated property",
            existing,
        );
    }
    if existing.expression.is_none() {
        existing.expression = incoming.expression.take();
    }
    if existing.value.is_none() {
        existing.value = incoming.value.take();
    }
    if existing.description.is_none() {
        existing.description = incoming.description.take();
    }
    existing.declarations.extend(incoming.declarations);
    let mut index: BTreeMap<_, _> = existing
        .children
        .iter()
        .enumerate()
        .filter(|(_, n)| n.kind != SyntaxKind::Reference)
        .map(|(i, n)| ((n.keyword.to_ascii_lowercase(), n.name.clone()), i))
        .collect();
    for child in incoming.children {
        let key = (child.keyword.to_ascii_lowercase(), child.name.clone());
        if let Some(i) = index
            .get(&key)
            .copied()
            .filter(|_| child.kind != SyntaxKind::Reference)
        {
            let previous = &mut existing.children[i];
            if child.name.is_some()
                && previous.kind == SyntaxKind::Object
                && child.kind == SyntaxKind::Object
            {
                merge_partial(previous, child, diags);
            } else {
                diag(
                    diags,
                    Code::TmdlDuplicateObject,
                    "Partial declaration repeats a property",
                    &child,
                );
                diag(
                    diags,
                    Code::TmdlDuplicateObject,
                    "Earlier declaration of the repeated property",
                    previous,
                );
            }
        } else {
            index.insert(key, existing.children.len());
            existing.children.push(child);
        }
    }
}

pub(crate) fn project(
    scope: &str,
    documents: &[SourceDocument],
    diags: &mut Vec<TmdlDiagnostic>,
) -> SemanticModel {
    let mut roots = Vec::new();
    for doc in documents.iter().filter(|d| d.is_valid) {
        collect_roots(&doc.nodes, &mut roots);
    }
    let mut merged: Vec<SyntaxNode> = Vec::new();
    let mut root_index = BTreeMap::<(String, Option<String>), usize>::new();
    let mut source_parts: BTreeMap<(String, Option<String>), Vec<SourceSpan>> = BTreeMap::new();
    let mut orders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for mut node in roots {
        let keyword = node.keyword.to_ascii_lowercase();
        let key = (keyword.clone(), node.name.clone());
        if node.kind == SyntaxKind::Reference {
            orders
                .entry(keyword.clone())
                .or_default()
                .push(node.name.clone().unwrap_or_default());
            if node.children.is_empty() {
                continue;
            }
            node.kind = SyntaxKind::Object;
        }
        source_parts
            .entry(key.clone())
            .or_default()
            .push(node.span.clone());
        if let Some(index) = root_index.get(&key) {
            let existing = &mut merged[*index];
            merge_partial(existing, node, diags);
        } else {
            root_index.insert(key, merged.len());
            merged.push(node);
        }
    }
    for node in &merged {
        validate_duplicates(node, diags);
    }
    for kind in ["database", "model"] {
        for node in merged
            .iter()
            .filter(|n| n.keyword.eq_ignore_ascii_case(kind))
            .skip(1)
        {
            diag(
                diags,
                Code::TmdlDuplicateObject,
                "A semantic model cannot have multiple database/model identities",
                node,
            );
        }
    }
    let fallback = SyntaxNode {
        kind: SyntaxKind::Object,
        keyword: "model".into(),
        name: Some("Model".into()),
        value: None,
        expression: None,
        description: None,
        children: vec![],
        declarations: vec![],
        span: SourceSpan {
            file: String::new(),
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        },
        header: SourceSpan {
            file: String::new(),
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        },
    };
    let model_node = merged
        .iter()
        .find(|n| n.keyword.eq_ignore_ascii_case("model"))
        .unwrap_or(&fallback);
    if std::ptr::eq(model_node, &fallback) {
        diag(
            diags,
            Code::TmdlSyntaxError,
            "No model declaration found",
            model_node,
        );
    }
    let mut model = SemanticModel {
        metadata: metadata(model_node, scope, diags),
        database: None,
        tables: vec![],
        relationships: vec![],
        roles: vec![],
        perspectives: vec![],
        cultures: vec![],
        expressions: vec![],
        functions: vec![],
    };
    for node in &merged {
        match node.keyword.to_ascii_lowercase().as_str() {
            "database" => model.database = Some(metadata(node, scope, diags)),
            "model" => {}
            "table" => {
                let mut table = table(node, scope, diags);
                if let Some(parts) = source_parts.get(&("table".into(), node.name.clone())) {
                    table.metadata.sources = parts.clone();
                }
                model.tables.push(table);
            }
            "relationship" => model.relationships.push(relationship(node, scope, diags)),
            "role" => model.roles.push(role(node, scope, diags)),
            "perspective" => model.perspectives.push(perspective(node, scope, diags)),
            "culture" => model.cultures.push(culture(node, scope, diags)),
            "expression" => model.expressions.push(NamedExpression {
                metadata: metadata(node, scope, diags),
                kind: property(node, "kind"),
                expression: expression(node),
            }),
            "function" => model.functions.push(ModelFunction {
                metadata: metadata(node, scope, diags),
                expression: expression(node),
            }),
            "annotation" => model.metadata.annotations.push(Property {
                name: node.name.clone().unwrap_or_default(),
                value: node
                    .expression
                    .as_ref()
                    .map(|e| e.display.clone())
                    .unwrap_or_default(),
                source: node.span.clone(),
            }),
            _ => diag(
                diags,
                Code::TmdlUnsupportedObject,
                "Unsupported root object retained in source",
                node,
            ),
        }
    }
    if let Some(order) = orders.get("table") {
        model.tables.sort_by_key(|t| {
            order
                .iter()
                .position(|n| n == &t.metadata.name)
                .unwrap_or(usize::MAX)
        });
    }
    if let Some(order) = orders.get("role") {
        model.roles.sort_by_key(|t| {
            order
                .iter()
                .position(|n| n == &t.metadata.name)
                .unwrap_or(usize::MAX)
        });
    }
    if let Some(order) = orders.get("perspective") {
        model.perspectives.sort_by_key(|t| {
            order
                .iter()
                .position(|n| n == &t.metadata.name)
                .unwrap_or(usize::MAX)
        });
    }
    if let Some(order) = orders.get("culture") {
        model.cultures.sort_by_key(|t| {
            order
                .iter()
                .position(|n| n == &t.metadata.name)
                .unwrap_or(usize::MAX)
        });
    }
    resolve(&mut model, diags);
    model
}

fn table(node: &SyntaxNode, scope: &str, diags: &mut Vec<TmdlDiagnostic>) -> Table {
    let meta = metadata(node, scope, diags);
    let scope = &meta.id;
    let partitions: Vec<_> = children(node, "partition")
        .map(|n| Partition {
            metadata: metadata(n, scope, diags),
            mode: property(n, "mode"),
            source_kind: n.expression.as_ref().map(|e| e.display.trim().into()),
            expression: child_expression(n, "source").or_else(|| child_expression(n, "expression")),
        })
        .collect();
    let calculated_table = partitions.iter().any(|p| {
        p.source_kind
            .as_deref()
            .is_some_and(|v| v.eq_ignore_ascii_case("calculated"))
    });
    let columns = children(node, "column")
        .map(|n| {
            let expr = expression(n);
            Column {
                metadata: metadata(n, scope, diags),
                kind: if expr.is_some() {
                    ColumnKind::Calculated
                } else if calculated_table {
                    ColumnKind::CalculatedTable
                } else {
                    ColumnKind::Data
                },
                data_type: property(n, "dataType"),
                source_column: property(n, "sourceColumn"),
                expression: expr,
                format_string: property(n, "formatString"),
                sort_by_column: ref_property(n, "sortByColumn"),
            }
        })
        .collect();
    let measures = children(node, "measure")
        .map(|n| Measure {
            metadata: metadata(n, scope, diags),
            expression: expression(n),
            format_string: property(n, "formatString"),
            display_folder: property(n, "displayFolder"),
        })
        .collect();
    let hierarchies = children(node, "hierarchy")
        .map(|n| {
            let meta = metadata(n, scope, diags);
            let mut levels: Vec<_> = children(n, "level")
                .enumerate()
                .map(|(i, level)| HierarchyLevel {
                    metadata: metadata(level, &meta.id, diags),
                    ordinal: property(level, "ordinal")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(i),
                    column: ref_property(level, "column"),
                })
                .collect();
            levels.sort_by_key(|l| l.ordinal);
            Hierarchy {
                metadata: meta,
                levels,
            }
        })
        .collect();
    Table {
        metadata: meta,
        columns,
        measures,
        hierarchies,
        partitions,
    }
}
fn endpoint(
    node: &SyntaxNode,
    name: &str,
    diags: &mut Vec<TmdlDiagnostic>,
) -> Option<ObjectReference> {
    let value = property(node, name).and_then(|v| lexer::reference(&v).ok());
    match value {
        Some(parts) if parts.len() == 2 => Some(ObjectReference {
            table: parts[0].clone(),
            object: parts[1].clone(),
            resolved: false,
        }),
        _ => {
            diag(
                diags,
                Code::TmdlInvalidRelationshipReference,
                format!("Missing or invalid {name} reference"),
                node,
            );
            None
        }
    }
}
fn relationship(node: &SyntaxNode, scope: &str, diags: &mut Vec<TmdlDiagnostic>) -> Relationship {
    Relationship {
        metadata: metadata(node, scope, diags),
        from: endpoint(node, "fromColumn", diags),
        to: endpoint(node, "toColumn", diags),
        from_cardinality: property(node, "fromCardinality").unwrap_or_else(|| "many".into()),
        to_cardinality: property(node, "toCardinality").unwrap_or_else(|| "one".into()),
        cross_filter: property(node, "crossFilteringBehavior")
            .unwrap_or_else(|| "oneDirection".into()),
        is_active: property(node, "isActive").is_none_or(|v| !v.eq_ignore_ascii_case("false")),
        security_filter: property(node, "securityFilteringBehavior"),
    }
}
fn role(node: &SyntaxNode, scope: &str, diags: &mut Vec<TmdlDiagnostic>) -> Role {
    let meta = metadata(node, scope, diags);
    let filters = children(node, "tablePermission")
        .map(|n| {
            let filter_metadata = metadata(n, &meta.id, diags);
            let filter_scope = filter_metadata.id.clone();
            TableFilter {
                metadata: filter_metadata,
                table: n.name.clone().unwrap_or_default(),
                expression: expression(n).or_else(|| child_expression(n, "filterExpression")),
                resolved: false,
                column_permissions: children(n, "columnPermission")
                    .map(|c| ColumnPermission {
                        metadata: metadata(c, &filter_scope, diags),
                        permission: c
                            .expression
                            .as_ref()
                            .map(|e| e.display.clone())
                            .or_else(|| property(c, "metadataPermission")),
                    })
                    .collect(),
            }
        })
        .collect();
    let members = children(node, "member")
        .map(|n| RoleMember {
            metadata: metadata(n, &meta.id, diags),
            member_type: n.expression.as_ref().map(|e| e.display.clone()),
            identity_provider: property(n, "identityProvider"),
        })
        .collect();
    Role {
        metadata: meta,
        model_permission: property(node, "modelPermission"),
        filters,
        members,
    }
}
fn perspective(node: &SyntaxNode, scope: &str, diags: &mut Vec<TmdlDiagnostic>) -> Perspective {
    let meta = metadata(node, scope, diags);
    let tables = children(node, "perspectiveTable")
        .map(|n| {
            let names = |kind: &str, prop: &str| {
                n.children
                    .iter()
                    .filter(|c| c.keyword.eq_ignore_ascii_case(kind))
                    .filter_map(|c| ref_property(c, prop).or(c.name.clone()))
                    .collect()
            };
            PerspectiveTable {
                metadata: metadata(n, &meta.id, diags),
                table: ref_property(n, "table")
                    .or(n.name.clone())
                    .unwrap_or_default(),
                resolved: false,
                columns: names("perspectiveColumn", "column"),
                measures: names("perspectiveMeasure", "measure"),
                hierarchies: names("perspectiveHierarchy", "hierarchy"),
            }
        })
        .collect();
    Perspective {
        metadata: meta,
        tables,
    }
}
fn culture(node: &SyntaxNode, scope: &str, diags: &mut Vec<TmdlDiagnostic>) -> Culture {
    fn translations(node: &SyntaxNode, target: &str, result: &mut Vec<Translation>) {
        for child in &node.children {
            let target = format!(
                "{target}/{}:{}",
                child.keyword,
                child.name.as_deref().unwrap_or("")
            );
            let properties: Vec<_> = child
                .children
                .iter()
                .filter_map(|p| {
                    p.value.as_ref().map(|value| Property {
                        name: p.keyword.clone(),
                        value: value.clone(),
                        source: p.span.clone(),
                    })
                })
                .collect();
            if !properties.is_empty() {
                result.push(Translation {
                    target: target.clone(),
                    properties,
                    source: child.span.clone(),
                });
            }
            translations(child, &target, result);
        }
    }
    let mut records = Vec::new();
    translations(node, "", &mut records);
    Culture {
        metadata: metadata(node, scope, diags),
        translations: records,
        linguistic_metadata: child_expression(node, "linguisticMetadata"),
    }
}

fn resolve(model: &mut SemanticModel, diags: &mut Vec<TmdlDiagnostic>) {
    let columns: BTreeSet<_> = model
        .tables
        .iter()
        .flat_map(|t| {
            t.columns
                .iter()
                .map(|c| (t.metadata.name.clone(), c.metadata.name.clone()))
        })
        .collect();
    let issue = |diags: &mut Vec<TmdlDiagnostic>, code, message: String, meta: &ObjectMetadata| {
        diags.push(TmdlDiagnostic::new(
            Severity::Warning,
            code,
            message,
            meta.sources.first().cloned(),
        ));
    };
    for rel in &mut model.relationships {
        for endpoint in [&mut rel.from, &mut rel.to].into_iter().flatten() {
            endpoint.resolved =
                columns.contains(&(endpoint.table.clone(), endpoint.object.clone()));
            if !endpoint.resolved {
                issue(
                    diags,
                    Code::TmdlInvalidRelationshipReference,
                    "Unresolved relationship endpoint".into(),
                    &rel.metadata,
                );
            }
        }
    }
    for table in &model.tables {
        for col in &table.columns {
            if let Some(sort) = &col.sort_by_column {
                if !columns.contains(&(table.metadata.name.clone(), sort.clone())) {
                    issue(
                        diags,
                        Code::TmdlUnresolvedReference,
                        "Unresolved sort column".into(),
                        &col.metadata,
                    );
                }
            }
        }
        for hierarchy in &table.hierarchies {
            for level in &hierarchy.levels {
                if !level
                    .column
                    .as_ref()
                    .is_some_and(|c| columns.contains(&(table.metadata.name.clone(), c.clone())))
                {
                    issue(
                        diags,
                        Code::TmdlInvalidHierarchyReference,
                        "Unresolved hierarchy level column".into(),
                        &level.metadata,
                    );
                }
            }
        }
    }
    for role in &mut model.roles {
        for filter in &mut role.filters {
            filter.resolved = model.tables.iter().any(|t| t.metadata.name == filter.table);
            if !filter.resolved {
                issue(
                    diags,
                    Code::TmdlUnresolvedReference,
                    "Unresolved role table".into(),
                    &filter.metadata,
                );
            }
        }
    }
    for perspective in &mut model.perspectives {
        for included in &mut perspective.tables {
            let table = model
                .tables
                .iter()
                .find(|t| t.metadata.name == included.table);
            let valid = table.is_some_and(|t| {
                included
                    .columns
                    .iter()
                    .all(|n| t.columns.iter().any(|c| &c.metadata.name == n))
                    && included
                        .measures
                        .iter()
                        .all(|n| t.measures.iter().any(|m| &m.metadata.name == n))
                    && included
                        .hierarchies
                        .iter()
                        .all(|n| t.hierarchies.iter().any(|h| &h.metadata.name == n))
            });
            included.resolved = valid;
            if !valid {
                issue(
                    diags,
                    Code::TmdlUnresolvedReference,
                    "Unresolved perspective member".into(),
                    &included.metadata,
                );
            }
        }
    }
}
