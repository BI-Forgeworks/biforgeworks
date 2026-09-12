use super::{
    diagnostics::{TmdlCode, TmdlDiagnostic},
    lexer::{self, Line},
    syntax::*,
};
use crate::Severity;

pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 64;

pub fn parse_document(file: &str, text: String) -> (SourceDocument, Vec<TmdlDiagnostic>) {
    let mut diagnostics = Vec::new();
    if text.len() > MAX_SOURCE_BYTES
        || text
            .bytes()
            .filter(|byte| *byte == b'\n')
            .take(200_001)
            .count()
            > 200_000
        || text.contains('\0')
    {
        diagnostics.push(TmdlDiagnostic::new(
            Severity::Error,
            TmdlCode::TmdlLimitExceeded,
            "Source exceeds the byte/line limit or contains NUL",
            None,
        ));
        return (
            SourceDocument {
                file: file.into(),
                text,
                nodes: vec![],
                is_valid: false,
            },
            diagnostics,
        );
    }
    let lines = lexer::lines(&text);
    let mut parser = Parser {
        file,
        text: &text,
        lines,
        cursor: 0,
        count: 0,
        diagnostics: &mut diagnostics,
    };
    let nodes = parser.block(0);
    validate_scopes(&nodes, "", &mut diagnostics);
    (
        SourceDocument {
            file: file.into(),
            text,
            nodes,
            is_valid: !diagnostics.iter().any(|d| d.severity == Severity::Error),
        },
        diagnostics,
    )
}

struct Parser<'a, 'd> {
    file: &'a str,
    text: &'a str,
    lines: Vec<Line<'a>>,
    cursor: usize,
    count: usize,
    diagnostics: &'d mut Vec<TmdlDiagnostic>,
}

impl Parser<'_, '_> {
    fn span(&self, line: &Line<'_>, start: usize, end: usize) -> SourceSpan {
        SourceSpan {
            file: self.file.into(),
            start,
            end,
            line: line.number,
            column: self.text[line.start..start]
                .trim_start_matches('\u{feff}')
                .chars()
                .count()
                + 1,
        }
    }
    fn error(&mut self, line: &Line<'_>, message: &str) {
        self.diagnostics.push(TmdlDiagnostic::new(
            Severity::Error,
            TmdlCode::TmdlSyntaxError,
            message,
            Some(self.span(line, line.content_start, line.end)),
        ));
    }
    fn block(&mut self, depth: usize) -> Vec<SyntaxNode> {
        let mut nodes = Vec::new();
        let mut description: Vec<String> = Vec::new();
        while let Some(line) = self.lines.get(self.cursor).cloned() {
            if line.content.trim().is_empty() {
                if !description.is_empty() {
                    self.error(&line, "Description must immediately precede a declaration");
                    description.clear();
                }
                self.cursor += 1;
                continue;
            }
            if line.indent < depth {
                break;
            }
            if depth > MAX_DEPTH || self.count >= MAX_NODES {
                self.error(&line, "Syntax nesting or node limit exceeded");
                self.cursor = self.lines.len();
                break;
            }
            if !line.valid_indent || line.indent != depth {
                self.error(
                    &line,
                    "Inconsistent structural indentation; use tabs or a consistent space unit",
                );
                self.cursor += 1;
                continue;
            }
            if let Some(value) = line.content.strip_prefix("///") {
                description.push(value.strip_prefix(' ').unwrap_or(value).to_owned());
                self.cursor += 1;
                continue;
            }
            if line.content.starts_with("//") {
                self.error(
                    &line,
                    "Only triple-slash descriptions are supported outside expressions",
                );
                self.cursor += 1;
                continue;
            }
            self.cursor += 1;
            self.count += 1;
            let parsed = self.header(&line);
            let (kind, keyword, name, value, assignment) = match parsed {
                Ok(parsed) => parsed,
                Err(message) => {
                    self.error(&line, message);
                    description.clear();
                    continue;
                }
            };
            let header = self.span(&line, line.content_start, line.end);
            let expression = assignment.and_then(|offset| {
                self.expression(
                    &line,
                    offset,
                    name.is_some() && !keyword.eq_ignore_ascii_case("annotation"),
                )
            });
            let mut node = SyntaxNode {
                kind,
                keyword,
                name,
                value,
                expression,
                description: (!description.is_empty()).then(|| description.join("\n")),
                span: header.clone(),
                header,
                children: Vec::new(),
                declarations: Vec::new(),
            };
            description.clear();
            if self
                .lines
                .get(self.cursor)
                .is_some_and(|l| l.content.trim().is_empty() || l.indent > depth)
            {
                node.children = self.block(depth + 1);
            }
            if node.kind == SyntaxKind::Property && !node.children.is_empty() {
                self.error(
                    &line,
                    "A scalar colon property cannot contain child declarations",
                );
            }
            node.span.end = self
                .lines
                .get(self.cursor.saturating_sub(1))
                .map_or(line.end, |l| l.end);
            node.declarations.push(node.span.clone());
            nodes.push(node);
        }
        if !description.is_empty() {
            if let Some(line) = self.lines.last().cloned() {
                self.error(&line, "Description has no following declaration");
            }
        }
        nodes
    }
    #[allow(clippy::type_complexity)]
    fn header(
        &self,
        line: &Line<'_>,
    ) -> Result<
        (
            SyntaxKind,
            String,
            Option<String>,
            Option<String>,
            Option<usize>,
        ),
        &'static str,
    > {
        let (mut keyword, mut tail) = lexer::identifier(line.content)?;
        let mut kind = SyntaxKind::Object;
        if keyword.eq_ignore_ascii_case("ref") {
            (keyword, tail) = lexer::identifier(tail)?;
            kind = SyntaxKind::Reference;
        }
        let mut tail = tail.trim_start();
        let mut name = None;
        if !tail.is_empty() && !tail.starts_with([':', '=']) {
            let (identifier, remaining) =
                if keyword.eq_ignore_ascii_case("function") && !tail.starts_with('\'') {
                    let end = tail
                        .find(|c: char| c.is_whitespace() || matches!(c, '=' | ':'))
                        .unwrap_or(tail.len());
                    let name = &tail[..end];
                    if !name.split('.').all(|s| {
                        !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
                    }) {
                        return Err("Invalid function name");
                    }
                    (name.to_owned(), &tail[end..])
                } else {
                    lexer::identifier(tail)?
                };
            name = Some(identifier);
            tail = remaining.trim_start();
        }
        if kind == SyntaxKind::Reference && (name.is_none() || !tail.is_empty()) {
            return Err("Invalid ref declaration");
        }
        if name.as_deref() == Some("") {
            return Err("Object name cannot be empty");
        }
        if name.is_none()
            && !tail.starts_with(':')
            && matches!(
                keyword.to_ascii_lowercase().as_str(),
                "model"
                    | "table"
                    | "column"
                    | "measure"
                    | "hierarchy"
                    | "level"
                    | "partition"
                    | "relationship"
                    | "role"
                    | "perspective"
                    | "culture"
                    | "function"
            )
        {
            return Err("Named object declaration requires a name");
        }
        if let Some(rest) = tail.strip_prefix(':') {
            if name.is_some() {
                return Err("Object declaration cannot assign a colon property");
            }
            Ok((
                SyntaxKind::Property,
                keyword,
                name,
                Some(lexer::scalar(rest)?),
                None,
            ))
        } else if let Some(rest) = tail.strip_prefix('=') {
            let offset = line.content_start + line.content.len() - rest.len();
            Ok((kind, keyword, name, None, Some(offset)))
        } else if tail.is_empty() {
            Ok((kind, keyword, name, None, None))
        } else {
            Err("Unexpected declaration delimiter or unquoted name")
        }
    }
    fn expression(
        &mut self,
        line: &Line<'_>,
        offset: usize,
        object_default: bool,
    ) -> Option<Expression> {
        let end = line.start
            + self.text[line.start..line.end]
                .trim_end_matches(['\n', '\r'])
                .len();
        let inline = &self.text[offset..end];
        let trimmed = inline.trim();
        let fenced = trimmed == "```";
        if !trimmed.is_empty() && !fenced {
            let start = offset + inline.len() - inline.trim_start().len();
            return Some(Expression {
                raw: self.text[start..end].into(),
                display: self.text[start..end].into(),
                span: self.span(line, start, end),
                fenced: false,
            });
        }
        let start_index = self.cursor;
        let boundary = line.indent + if object_default { 2 } else { 1 };
        let mut closing_prefix = None;
        while let Some(next) = self.lines.get(self.cursor) {
            if fenced && next.content.trim_end() == "```" {
                closing_prefix = Some(&self.text[next.start..next.content_start]);
                break;
            }
            if !fenced && !next.content.trim().is_empty() && next.indent < boundary {
                break;
            }
            self.cursor += 1;
        }
        let stop_index = self.cursor;
        if fenced {
            if self.cursor == self.lines.len() {
                self.error(line, "Unclosed expression fence");
            } else {
                self.cursor += 1;
            }
        }
        if start_index == stop_index && !fenced {
            self.error(line, "Missing or under-indented expression body");
            return None;
        }
        let start = self.lines.get(start_index).map_or(line.end, |l| l.start);
        let stop = self
            .lines
            .get(stop_index)
            .map_or(self.text.len(), |l| l.start);
        let raw = &self.text[start..stop];
        let display = raw
            .split_inclusive('\n')
            .map(|s| {
                if let Some(prefix) = closing_prefix {
                    return s.strip_prefix(prefix).unwrap_or(s);
                }
                let mut bytes = 0;
                let mut levels = 0;
                while levels < boundary {
                    if s[bytes..].starts_with('\t') {
                        bytes += 1;
                    } else if s.as_bytes()[bytes..]
                        .iter()
                        .take(line.space_unit)
                        .filter(|b| **b == b' ')
                        .count()
                        == line.space_unit
                    {
                        bytes += line.space_unit;
                    } else {
                        break;
                    }
                    levels += 1;
                }
                &s[bytes..]
            })
            .collect::<String>();
        let body_line = self.lines.get(start_index).unwrap_or(line);
        Some(Expression {
            raw: raw.into(),
            display,
            span: self.span(body_line, start, stop),
            fenced,
        })
    }
}

/// Validate only known containment rules. Unknown extension subtrees stay opaque.
fn validate_scopes(nodes: &[SyntaxNode], parent: &str, diagnostics: &mut Vec<TmdlDiagnostic>) {
    for node in nodes {
        if node.kind == SyntaxKind::Property {
            continue;
        }
        let keyword = node.keyword.to_ascii_lowercase();
        let allowed: Option<&[&str]> = match keyword.as_str() {
            "database" => Some(&[""]),
            "model" => Some(&["", "database"]),
            "table" | "relationship" | "role" | "perspective" | "culture" | "function" => {
                Some(&["", "model"])
            }
            "column" | "measure" | "hierarchy" | "partition" => Some(&["table"]),
            "level" => Some(&["hierarchy"]),
            "tablepermission" | "member" => Some(&["role"]),
            "columnpermission" => Some(&["tablepermission"]),
            "perspectivetable" => Some(&["perspective"]),
            "perspectivecolumn" | "perspectivemeasure" | "perspectivehierarchy" => {
                Some(&["perspectivetable"])
            }
            _ => None,
        };
        if let Some(parents) = allowed {
            if !parents.contains(&parent) {
                diagnostics.push(TmdlDiagnostic::new(
                    Severity::Error,
                    TmdlCode::TmdlSyntaxError,
                    "Object is declared in an invalid parent scope",
                    Some(node.header.clone()),
                ));
            }
            validate_scopes(&node.children, &keyword, diagnostics);
        }
    }
}
