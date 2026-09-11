//! Outer metadata interpretation for `.pbip`, `definition.pbir`,
//! `definition.pbism`, and PBIR `definition/version.json`.
//!
//! Only the documented envelope properties are inspected (`$schema`,
//! `version`, `artifacts[].report.path`, `datasetReference`). Values that
//! could carry sensitive data, such as `byConnection.connectionString`, are
//! never read into typed results.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

const SCHEMA_BASE: &str = "https://developer.microsoft.com/json-schemas/";
pub(crate) const PBIP_SCHEMA_FAMILY: &str = "fabric/pbip/pbipProperties";
pub(crate) const REPORT_DEFINITION_SCHEMA_FAMILY: &str = "fabric/item/report/definitionProperties";
pub(crate) const MODEL_DEFINITION_SCHEMA_FAMILY: &str =
    "fabric/item/semanticModel/definitionProperties";
pub(crate) const VERSION_METADATA_SCHEMA_FAMILY: &str =
    "fabric/item/report/definition/versionMetadata";

pub(crate) type Object = Map<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JsonError {
    InvalidUtf8,
    /// Syntax error, or a duplicate object key (which would make the
    /// document ambiguous between readers). Only the position is kept so
    /// that no document content can leak into diagnostics.
    InvalidJson {
        line: usize,
        column: usize,
        duplicate_key: bool,
    },
    NotAnObject,
}

impl JsonError {
    pub(crate) fn describe(self) -> String {
        match self {
            Self::InvalidUtf8 => "is not valid UTF-8 text".to_owned(),
            Self::InvalidJson {
                line,
                column,
                duplicate_key: true,
            } => format!("contains a duplicate JSON object key (line {line}, column {column})"),
            Self::InvalidJson { line, column, .. } => {
                format!("is not valid JSON (line {line}, column {column})")
            }
            Self::NotAnObject => "is not a JSON object".to_owned(),
        }
    }
}

/// Parses a metadata file into a JSON object, rejecting invalid UTF-8,
/// syntax errors, and duplicate keys. A leading UTF-8 BOM is tolerated.
pub(crate) fn parse_object(bytes: &[u8]) -> Result<Object, JsonError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| JsonError::InvalidUtf8)?;
    match serde_json::from_str::<StrictValue>(text) {
        Ok(StrictValue(Value::Object(object))) => Ok(object),
        Ok(_) => Err(JsonError::NotAnObject),
        Err(err) => Err(JsonError::InvalidJson {
            line: err.line(),
            column: err.column(),
            duplicate_key: err.classify() == serde_json::error::Category::Data,
        }),
    }
}

/// A `serde_json::Value` that fails to deserialize on duplicate object keys.
struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor).map(StrictValue)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Number::from_f64(v).map_or(Value::Null, Value::Number))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(StrictValue(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom("duplicate key"));
            }
            let StrictValue(value) = map.next_value()?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaStatus {
    Absent,
    Supported,
    /// Not a string, not the documented URL shape, or an unknown major
    /// version. Never treated as validated.
    Unsupported,
}

/// Checks `$schema` against `https://developer.microsoft.com/json-schemas/
/// <family>/<major>.<minor>.<patch>/schema.json` with a known major.
pub(crate) fn schema_status(object: &Object, family: &str, majors: &[u64]) -> SchemaStatus {
    let Some(value) = object.get("$schema") else {
        return SchemaStatus::Absent;
    };
    let supported = value
        .as_str()
        .and_then(|url| url.strip_prefix(SCHEMA_BASE))
        .and_then(|rest| rest.strip_prefix(family))
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.strip_suffix("/schema.json"))
        .and_then(|version| parse_numeric_version(version, 3))
        .is_some_and(|parts| majors.contains(&parts[0]));
    if supported {
        SchemaStatus::Supported
    } else {
        SchemaStatus::Unsupported
    }
}

/// Parses exactly `count` dot-separated unsigned decimal components.
fn parse_numeric_version(text: &str, count: usize) -> Option<Vec<u64>> {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != count {
        return None;
    }
    parts
        .iter()
        .map(|part| {
            if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                part.parse().ok()
            }
        })
        .collect()
}

/// Which storage formats an item's `definition.pbir` / `definition.pbism`
/// `version` permits, per Microsoft's documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionVersion {
    /// `1.0`: PBIR-Legacy `report.json` only, or TMSL `model.bim` only.
    LegacyOnly,
    /// `4.x`: legacy or `definition/` folder formats.
    Modern,
    /// Missing, malformed, or a version whose format rules are not known.
    Unsupported,
}

pub(crate) fn definition_version(object: &Object) -> DefinitionVersion {
    match object
        .get("version")
        .and_then(Value::as_str)
        .and_then(|v| parse_numeric_version(v, 2))
        .as_deref()
    {
        Some([1, 0]) => DefinitionVersion::LegacyOnly,
        Some([4, _]) => DefinitionVersion::Modern,
        _ => DefinitionVersion::Unsupported,
    }
}

/// `.pbip` `version` is documented only as `1.0`.
pub(crate) fn pbip_version_supported(object: &Object) -> bool {
    matches!(
        object
            .get("version")
            .and_then(Value::as_str)
            .and_then(|v| parse_numeric_version(v, 2))
            .as_deref(),
        Some([1, _])
    )
}

/// PBIR `definition/version.json` `version`: `major.minor.0` with a known
/// major (`1` and `2` are the published PBIR definition versions).
pub(crate) fn pbir_version_supported(object: &Object) -> bool {
    object
        .get("version")
        .and_then(Value::as_str)
        .filter(|v| {
            !v.split('.')
                .any(|part| part.len() > 1 && part.starts_with('0'))
        })
        .and_then(|v| parse_numeric_version(v, 3))
        .is_some_and(|parts| matches!(parts[..], [1 | 2, _, 0]))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReportReference {
    Path(String),
    Missing,
    /// More than one report artifact; the target cannot be chosen safely.
    Ambiguous,
    /// `artifacts` or the report entry has the wrong shape.
    Invalid,
}

/// Reads `artifacts[].report.path`. Also returns how many artifact entries
/// are not report references (unsupported item kinds).
pub(crate) fn pbip_report_reference(object: &Object) -> (ReportReference, usize) {
    let artifacts = match object.get("artifacts") {
        None | Some(Value::Null) => return (ReportReference::Missing, 0),
        Some(Value::Array(items)) => items,
        Some(_) => return (ReportReference::Invalid, 0),
    };
    let mut reports = Vec::new();
    let mut other = 0;
    for artifact in artifacts {
        match artifact.as_object().and_then(|a| a.get("report")) {
            Some(report) => reports.push(report),
            None => other += 1,
        }
    }
    let reference = match reports.as_slice() {
        [] => ReportReference::Missing,
        [report] => match report.get("path").and_then(Value::as_str) {
            Some(path) => ReportReference::Path(path.to_owned()),
            None => ReportReference::Invalid,
        },
        _ => ReportReference::Ambiguous,
    };
    (reference, other)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DatasetReference {
    ByPath(String),
    /// Remote model. The connection details are deliberately not captured.
    ByConnection,
    Missing,
    /// Both `byPath` and `byConnection` are set.
    Ambiguous,
    Invalid,
}

/// Reads `datasetReference`, which must set exactly one of `byPath` and
/// `byConnection` to a non-null value.
pub(crate) fn dataset_reference(object: &Object) -> DatasetReference {
    let reference = match object.get("datasetReference") {
        None | Some(Value::Null) => return DatasetReference::Missing,
        Some(Value::Object(reference)) => reference,
        Some(_) => return DatasetReference::Invalid,
    };
    let by_path = reference.get("byPath").filter(|v| !v.is_null());
    let by_connection = reference.get("byConnection").filter(|v| !v.is_null());
    match (by_path, by_connection) {
        (Some(_), Some(_)) => DatasetReference::Ambiguous,
        (Some(by_path), None) => match by_path.get("path").and_then(Value::as_str) {
            Some(path) => DatasetReference::ByPath(path.to_owned()),
            None => DatasetReference::Invalid,
        },
        (None, Some(_)) => DatasetReference::ByConnection,
        (None, None) => DatasetReference::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(text: &str) -> Object {
        parse_object(text.as_bytes()).expect("test JSON is a valid object")
    }

    #[test]
    fn parse_rejects_invalid_utf8_json_and_non_objects() {
        assert_eq!(
            parse_object(b"{\"a\": \"\xFF\"}"),
            Err(JsonError::InvalidUtf8)
        );
        assert!(matches!(
            parse_object(b"{\"a\": "),
            Err(JsonError::InvalidJson {
                duplicate_key: false,
                ..
            })
        ));
        assert_eq!(parse_object(b"[1, 2]"), Err(JsonError::NotAnObject));
        assert_eq!(parse_object(b"null"), Err(JsonError::NotAnObject));
    }

    #[test]
    fn parse_rejects_duplicate_keys_at_any_depth() {
        for text in [
            r#"{"version": "4.0", "version": "1.0"}"#,
            r#"{"datasetReference": {"byPath": {"path": "a", "path": "b"}}}"#,
        ] {
            assert!(
                matches!(
                    parse_object(text.as_bytes()),
                    Err(JsonError::InvalidJson {
                        duplicate_key: true,
                        ..
                    })
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn parse_tolerates_bom_and_survives_deep_nesting() {
        assert!(parse_object(b"\xEF\xBB\xBF{}").is_ok());
        let deep = format!("{{\"a\":{}{}}}", "[".repeat(100_000), "]".repeat(100_000));
        assert!(matches!(
            parse_object(deep.as_bytes()),
            Err(JsonError::InvalidJson { .. })
        ));
    }

    #[test]
    fn invalid_json_description_does_not_echo_content() {
        let err = parse_object(br#"{"secret": "Password=hunter2" oops}"#).unwrap_err();
        assert!(!err.describe().contains("hunter2"));
    }

    #[test]
    fn schema_status_requires_known_family_and_major() {
        let family = REPORT_DEFINITION_SCHEMA_FAMILY;
        let url = |v: &str| {
            format!(
                r#"{{"$schema": "https://developer.microsoft.com/json-schemas/{family}/{v}/schema.json"}}"#
            )
        };
        assert_eq!(
            schema_status(&object(&url("2.0.0")), family, &[1, 2]),
            SchemaStatus::Supported
        );
        assert_eq!(
            schema_status(&object(&url("1.3.12")), family, &[1, 2]),
            SchemaStatus::Supported
        );
        assert_eq!(
            schema_status(&object(&url("3.0.0")), family, &[1, 2]),
            SchemaStatus::Unsupported
        );
        assert_eq!(
            schema_status(&object(&url("2.0")), family, &[1, 2]),
            SchemaStatus::Unsupported
        );
        assert_eq!(
            schema_status(&object(r#"{"$schema": 7}"#), family, &[1, 2]),
            SchemaStatus::Unsupported
        );
        assert_eq!(
            schema_status(&object(&url("2.0.0")), MODEL_DEFINITION_SCHEMA_FAMILY, &[1]),
            SchemaStatus::Unsupported
        );
        assert_eq!(
            schema_status(&object("{}"), family, &[1, 2]),
            SchemaStatus::Absent
        );
    }

    #[test]
    fn definition_versions_follow_documented_rules() {
        let v = |text: &str| definition_version(&object(text));
        assert_eq!(v(r#"{"version": "1.0"}"#), DefinitionVersion::LegacyOnly);
        assert_eq!(v(r#"{"version": "4.0"}"#), DefinitionVersion::Modern);
        assert_eq!(v(r#"{"version": "4.2"}"#), DefinitionVersion::Modern);
        for unsupported in [
            r#"{"version": "1.1"}"#,
            r#"{"version": "2.0"}"#,
            r#"{"version": "5.0"}"#,
            r#"{"version": "4"}"#,
            r#"{"version": "4.0.0"}"#,
            r#"{"version": "v4.0"}"#,
            r#"{"version": 4.0}"#,
            r#"{}"#,
        ] {
            assert_eq!(
                v(unsupported),
                DefinitionVersion::Unsupported,
                "{unsupported}"
            );
        }
    }

    #[test]
    fn pbir_version_metadata_requires_known_major_and_zero_patch() {
        let ok = |text: &str| pbir_version_supported(&object(text));
        assert!(ok(r#"{"version": "2.0.0"}"#));
        assert!(ok(r#"{"version": "1.0.0"}"#));
        assert!(ok(r#"{"version": "2.1.0"}"#));
        assert!(!ok(r#"{"version": "3.0.0"}"#));
        assert!(!ok(r#"{"version": "2.0.1"}"#));
        assert!(!ok(r#"{"version": "02.0.0"}"#));
        assert!(!ok(r#"{"version": "2.0"}"#));
        assert!(!ok(r#"{}"#));
    }

    #[test]
    fn pbip_report_reference_shapes() {
        let r = |text: &str| pbip_report_reference(&object(text));
        assert_eq!(
            r(r#"{"artifacts": [{"report": {"path": "A.Report"}}]}"#),
            (ReportReference::Path("A.Report".into()), 0)
        );
        assert_eq!(r(r#"{"artifacts": []}"#), (ReportReference::Missing, 0));
        assert_eq!(r(r#"{}"#), (ReportReference::Missing, 0));
        assert_eq!(r(r#"{"artifacts": {}}"#), (ReportReference::Invalid, 0));
        assert_eq!(
            r(r#"{"artifacts": [{"report": {"path": 3}}]}"#),
            (ReportReference::Invalid, 0)
        );
        assert_eq!(
            r(r#"{"artifacts": [{"report": {"path": "A"}}, {"report": {"path": "B"}}]}"#),
            (ReportReference::Ambiguous, 0)
        );
        assert_eq!(
            r(r#"{"artifacts": [{"dashboard": {}}, 5, {"report": {"path": "A"}}]}"#),
            (ReportReference::Path("A".into()), 2)
        );
    }

    #[test]
    fn dataset_reference_shapes() {
        let d = |text: &str| dataset_reference(&object(text));
        assert_eq!(
            d(r#"{"datasetReference": {"byPath": {"path": "../M"}}}"#),
            DatasetReference::ByPath("../M".into())
        );
        assert_eq!(
            d(r#"{"datasetReference": {"byPath": {"path": "../M"}, "byConnection": null}}"#),
            DatasetReference::ByPath("../M".into())
        );
        assert_eq!(
            d(
                r#"{"datasetReference": {"byPath": null, "byConnection": {"connectionString": "x"}}}"#
            ),
            DatasetReference::ByConnection
        );
        assert_eq!(
            d(r#"{"datasetReference": {"byPath": {"path": "a"}, "byConnection": {}}}"#),
            DatasetReference::Ambiguous
        );
        assert_eq!(d(r#"{"datasetReference": {}}"#), DatasetReference::Missing);
        assert_eq!(d(r#"{}"#), DatasetReference::Missing);
        assert_eq!(d(r#"{"datasetReference": []}"#), DatasetReference::Invalid);
        assert_eq!(
            d(r#"{"datasetReference": {"byPath": {"path": null}}}"#),
            DatasetReference::Invalid
        );
    }
}
