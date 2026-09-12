//! Structural line/token scanning; expression bodies never enter the token scanner.
#[derive(Debug, Clone)]
pub(crate) struct Line<'a> {
    pub start: usize,
    pub end: usize,
    pub content_start: usize,
    pub content: &'a str,
    pub indent: usize,
    pub valid_indent: bool,
    pub number: usize,
    pub space_unit: usize,
}

fn space_unit(text: &str) -> usize {
    let mut previous = "";
    let mut fenced = false;
    for line in text.lines() {
        let content = line.trim();
        if fenced {
            if content == "```" {
                fenced = false;
            }
            continue;
        }
        if content.ends_with("= ```") {
            fenced = true;
        }
        if content.is_empty() {
            continue;
        }
        let width = line.bytes().take_while(|b| *b == b' ').count();
        if width > 0 && !line[width..].starts_with('\t') {
            let keyword = previous
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            let factor = if previous.ends_with('=')
                && matches!(
                    keyword.as_str(),
                    "measure" | "column" | "function" | "expression" | "tablepermission"
                ) {
                2
            } else {
                1
            };
            return (width / factor).max(1);
        }
        if line.starts_with('\t') {
            return 4;
        }
        previous = content;
    }
    4
}

pub(crate) fn lines(text: &str) -> Vec<Line<'_>> {
    let mut offset = 0;
    let space_unit = space_unit(text);
    text.split_inclusive('\n')
        .enumerate()
        .map(|(i, raw)| {
            let body = raw.trim_end_matches(['\r', '\n']);
            let bom = if i == 0 && body.starts_with('\u{feff}') {
                3
            } else {
                0
            };
            let prefix = body[bom..]
                .bytes()
                .take_while(|b| matches!(b, b' ' | b'\t'))
                .count();
            let ws = &body[bom..bom + prefix];
            let tabs = ws.bytes().filter(|b| *b == b'\t').count();
            let spaces = ws.len() - tabs;
            let line = Line {
                start: offset,
                end: offset + raw.len(),
                content_start: offset + bom + prefix,
                content: &body[bom + prefix..],
                indent: tabs + spaces / space_unit,
                valid_indent: (tabs == 0 || spaces == 0) && spaces.is_multiple_of(space_unit),
                number: i + 1,
                space_unit,
            };
            offset += raw.len();
            line
        })
        .collect()
}

/// Read one identifier and leave delimiters to the caller. Doubled single quotes escape.
pub(crate) fn identifier(input: &str) -> Result<(String, &str), &'static str> {
    let input = input.trim_start();
    if let Some(rest) = input.strip_prefix('\'') {
        let mut value = String::new();
        let mut iter = rest.char_indices().peekable();
        while let Some((i, c)) = iter.next() {
            if c == '\'' {
                if iter.peek().is_some_and(|(_, next)| *next == '\'') {
                    iter.next();
                    value.push('\'');
                } else {
                    return Ok((value, &rest[i + 1..]));
                }
            } else {
                value.push(c);
            }
        }
        Err("Unclosed quoted identifier")
    } else {
        let end = input
            .find(|c: char| c.is_whitespace() || matches!(c, '.' | '=' | ':' | '\''))
            .unwrap_or(input.len());
        if end == 0 {
            Err("Expected an identifier")
        } else {
            Ok((input[..end].to_owned(), &input[end..]))
        }
    }
}

pub(crate) fn reference(input: &str) -> Result<Vec<String>, &'static str> {
    let mut rest = input;
    let mut result = Vec::new();
    loop {
        let (name, tail) = identifier(rest)?;
        result.push(name);
        rest = tail.trim();
        if rest.is_empty() {
            return Ok(result);
        }
        rest = rest
            .strip_prefix('.')
            .ok_or("Invalid qualified reference")?;
    }
}

pub(crate) fn scalar(input: &str) -> Result<String, &'static str> {
    let input = input.trim();
    if let Some(rest) = input.strip_prefix('"') {
        let mut value = String::new();
        let mut iter = rest.char_indices().peekable();
        while let Some((i, c)) = iter.next() {
            if c == '"' {
                if iter.peek().is_some_and(|(_, next)| *next == '"') {
                    iter.next();
                    value.push('"');
                } else if rest[i + 1..].trim().is_empty() {
                    return Ok(value);
                } else {
                    return Err("Unexpected text after quoted property");
                }
            } else {
                value.push(c);
            }
        }
        Err("Unclosed quoted property")
    } else {
        Ok(input.to_owned())
    }
}
