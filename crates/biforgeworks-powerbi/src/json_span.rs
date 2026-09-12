//! Byte-span location inside an already-validated JSON document.
//!
//! Editing a `.pbip` must change nothing but the one value being edited, so
//! the edit is performed as a byte splice over the value's exact span. This
//! scanner finds that span; it deliberately does not reserialize, so byte
//! order marks, indentation, newline style, key order, trailing newlines,
//! and unknown properties all survive untouched.
//!
//! Callers parse the document with [`crate::metadata::parse_object`] first,
//! which rejects malformed JSON and duplicate keys; this scanner re-checks
//! the structure it walks and refuses anything it does not understand.

use std::ops::Range;

/// Deepest object nesting this scanner will walk.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpanError {
    /// The document, or a value along the path, is not a JSON object.
    NotAnObject,
    /// A path element is absent.
    MemberMissing,
    /// A key along the path occurs more than once.
    DuplicateKey,
    /// The located value is not the expected `true`/`false` token.
    NotABoolean,
    /// Malformed input, or nesting deeper than this scanner walks.
    Malformed,
}

/// The UTF-8 byte order mark, which Power BI Desktop does not write but
/// other editors may; it is preserved rather than stripped.
pub(crate) const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Locates the boolean at `path` (a chain of object keys) and returns its
/// byte span within `bytes` together with its current value.
///
/// Spans are absolute offsets into `bytes`, including any leading BOM.
pub(crate) fn boolean_span(bytes: &[u8], path: &[&str]) -> Result<(Range<usize>, bool), SpanError> {
    let start = if bytes.starts_with(BOM) { BOM.len() } else { 0 };
    let mut cursor = Cursor {
        bytes,
        position: skip_whitespace(bytes, start),
    };
    let mut span = cursor.value_span(0)?;

    for key in path {
        let object = span.clone();
        span = member_span(bytes, object, key)?;
    }

    match &bytes[span.clone()] {
        b"true" => Ok((span, true)),
        b"false" => Ok((span, false)),
        _ => Err(SpanError::NotABoolean),
    }
}

/// Replaces `span` with `replacement`, leaving every other byte untouched.
pub(crate) fn splice(bytes: &[u8], span: Range<usize>, replacement: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + replacement.len());
    out.extend_from_slice(&bytes[..span.start]);
    out.extend_from_slice(replacement);
    out.extend_from_slice(&bytes[span.end..]);
    out
}

/// Finds `key` among the members of the object occupying `object`.
fn member_span(bytes: &[u8], object: Range<usize>, key: &str) -> Result<Range<usize>, SpanError> {
    if bytes.get(object.start) != Some(&b'{') {
        return Err(SpanError::NotAnObject);
    }
    let mut cursor = Cursor {
        bytes,
        position: skip_whitespace(bytes, object.start + 1),
    };
    let mut found: Option<Range<usize>> = None;

    if cursor.peek() == Some(b'}') {
        return Err(SpanError::MemberMissing);
    }
    loop {
        let name = cursor.string()?;
        cursor.skip_whitespace();
        if cursor.take() != Some(b':') {
            return Err(SpanError::Malformed);
        }
        cursor.skip_whitespace();
        let value = cursor.value_span(0)?;
        if name == key {
            if found.is_some() {
                return Err(SpanError::DuplicateKey);
            }
            found = Some(value);
        }
        cursor.skip_whitespace();
        match cursor.take() {
            Some(b',') => cursor.skip_whitespace(),
            Some(b'}') => break,
            _ => return Err(SpanError::Malformed),
        }
    }
    found.ok_or(SpanError::MemberMissing)
}

fn skip_whitespace(bytes: &[u8], mut position: usize) -> usize {
    while matches!(bytes.get(position), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        position += 1;
    }
    position
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }

    fn skip_whitespace(&mut self) {
        self.position = skip_whitespace(self.bytes, self.position);
    }

    /// Reads a JSON string, returning its decoded value (so that escaped
    /// keys such as `"settings"` compare equal to `settings`).
    fn string(&mut self) -> Result<String, SpanError> {
        if self.take() != Some(b'"') {
            return Err(SpanError::Malformed);
        }
        let mut units: Vec<u16> = Vec::new();
        loop {
            match self.take().ok_or(SpanError::Malformed)? {
                b'"' => break,
                b'\\' => {
                    let escape = self.take().ok_or(SpanError::Malformed)?;
                    let unit = match escape {
                        b'"' => u16::from(b'"'),
                        b'\\' => u16::from(b'\\'),
                        b'/' => u16::from(b'/'),
                        b'b' => 0x08,
                        b'f' => 0x0C,
                        b'n' => u16::from(b'\n'),
                        b'r' => u16::from(b'\r'),
                        b't' => u16::from(b'\t'),
                        b'u' => self.hex4()?,
                        _ => return Err(SpanError::Malformed),
                    };
                    units.push(unit);
                }
                byte if byte < 0x20 => return Err(SpanError::Malformed),
                byte => {
                    // Re-encode the raw UTF-8 sequence as UTF-16 units.
                    let length = utf8_length(byte)?;
                    let start = self.position - 1;
                    self.position = start + length;
                    let raw = self
                        .bytes
                        .get(start..self.position)
                        .ok_or(SpanError::Malformed)?;
                    let text = std::str::from_utf8(raw).map_err(|_| SpanError::Malformed)?;
                    units.extend(text.encode_utf16());
                }
            }
        }
        String::from_utf16(&units).map_err(|_| SpanError::Malformed)
    }

    fn hex4(&mut self) -> Result<u16, SpanError> {
        let digits = self
            .bytes
            .get(self.position..self.position + 4)
            .ok_or(SpanError::Malformed)?;
        let text = std::str::from_utf8(digits).map_err(|_| SpanError::Malformed)?;
        let unit = u16::from_str_radix(text, 16).map_err(|_| SpanError::Malformed)?;
        self.position += 4;
        Ok(unit)
    }

    /// Returns the span of the value starting at the cursor and leaves the
    /// cursor just past it.
    fn value_span(&mut self, depth: usize) -> Result<Range<usize>, SpanError> {
        if depth > MAX_DEPTH {
            return Err(SpanError::Malformed);
        }
        let start = self.position;
        match self.peek().ok_or(SpanError::Malformed)? {
            b'{' => self.skip_container(b'{', b'}', depth)?,
            b'[' => self.skip_container(b'[', b']', depth)?,
            b'"' => {
                self.string()?;
            }
            b't' | b'f' | b'n' => {
                while matches!(self.peek(), Some(b'a'..=b'z')) {
                    self.position += 1;
                }
            }
            b'-' | b'0'..=b'9' => {
                while matches!(
                    self.peek(),
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    self.position += 1;
                }
            }
            _ => return Err(SpanError::Malformed),
        }
        if self.position == start {
            return Err(SpanError::Malformed);
        }
        Ok(start..self.position)
    }

    fn skip_container(&mut self, open: u8, close: u8, depth: usize) -> Result<(), SpanError> {
        if self.take() != Some(open) {
            return Err(SpanError::Malformed);
        }
        self.skip_whitespace();
        if self.peek() == Some(close) {
            self.position += 1;
            return Ok(());
        }
        loop {
            if open == b'{' {
                self.string()?;
                self.skip_whitespace();
                if self.take() != Some(b':') {
                    return Err(SpanError::Malformed);
                }
                self.skip_whitespace();
            }
            self.value_span(depth + 1)?;
            self.skip_whitespace();
            match self.take() {
                Some(byte) if byte == close => return Ok(()),
                Some(b',') => self.skip_whitespace(),
                _ => return Err(SpanError::Malformed),
            }
        }
    }
}

fn utf8_length(first: u8) -> Result<usize, SpanError> {
    match first {
        0x00..=0x7F => Ok(1),
        0xC2..=0xDF => Ok(2),
        0xE0..=0xEF => Ok(3),
        0xF0..=0xF4 => Ok(4),
        _ => Err(SpanError::Malformed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &[&str] = &["settings", "enableAutoRecovery"];

    fn located(text: &str) -> Result<(Range<usize>, bool), SpanError> {
        boolean_span(text.as_bytes(), PATH)
    }

    #[test]
    fn locates_boolean_and_splices_only_that_token() {
        let text = "{\r\n  \"version\": \"1.0\",\r\n  \"settings\": {\r\n    \"enableAutoRecovery\": true\r\n  }\r\n}\r\n";
        let (span, value) = located(text).unwrap();
        assert!(value);
        assert_eq!(&text.as_bytes()[span.clone()], b"true");
        let spliced = splice(text.as_bytes(), span, b"false");
        assert_eq!(
            String::from_utf8(spliced).unwrap(),
            text.replace("true", "false")
        );
    }

    #[test]
    fn preserves_bom_and_reports_absolute_spans() {
        let text = "\u{FEFF}{\"settings\":{\"enableAutoRecovery\":false}}";
        let (span, value) = located(text).unwrap();
        assert!(!value);
        assert_eq!(&text.as_bytes()[span.clone()], b"false");
        let spliced = splice(text.as_bytes(), span, b"true");
        assert!(spliced.starts_with(BOM));
        assert_eq!(
            String::from_utf8(spliced).unwrap(),
            "\u{FEFF}{\"settings\":{\"enableAutoRecovery\":true}}"
        );
    }

    #[test]
    fn tolerates_unknown_properties_nesting_and_escapes() {
        let text = r#"{
  "$schema": "https://example/schema.json",
  "unknownArray": [1, -2.5e10, {"enableAutoRecovery": "decoy"}, [true]],
  "unknownObject": {"nested": {"deep": null}},
  "settings": {
    "qnaEnabled": true,
    "enableAutoRecovery": false,
    "trailing": "}\" tricky \\ value"
  },
  "after": "kept"
}"#;
        let path = &["settings", "enableAutoRecovery"];
        let (span, value) = boolean_span(text.as_bytes(), path).unwrap();
        assert!(!value);
        let spliced = splice(text.as_bytes(), span, b"true");
        let text_out = String::from_utf8(spliced).unwrap();
        assert!(text_out.contains(r#""enableAutoRecovery": true"#));
        assert!(text_out.contains(r#""decoy""#));
        assert!(text_out.contains(r#""}\" tricky \\ value""#));
        assert_eq!(text_out.len(), text.len() - 1);
    }

    #[test]
    fn handles_multi_byte_strings() {
        let text = "{\"名前\": \"データ€\", \"settings\": {\"enableAutoRecovery\": true}}";
        let (span, _) = located(text).unwrap();
        assert_eq!(&text.as_bytes()[span], b"true");
    }

    #[test]
    fn rejects_missing_wrong_typed_and_duplicate_members() {
        assert_eq!(located("{}"), Err(SpanError::MemberMissing));
        assert_eq!(located("[]"), Err(SpanError::NotAnObject));
        assert_eq!(
            located(r#"{"settings": {"other": true}}"#),
            Err(SpanError::MemberMissing)
        );
        assert_eq!(
            located(r#"{"settings": null}"#),
            Err(SpanError::NotAnObject)
        );
        assert_eq!(
            located(r#"{"settings": {"enableAutoRecovery": "true"}}"#),
            Err(SpanError::NotABoolean)
        );
        assert_eq!(
            located(r#"{"settings": {"enableAutoRecovery": 1}}"#),
            Err(SpanError::NotABoolean)
        );
        assert_eq!(
            located(r#"{"settings": {"enableAutoRecovery": null}}"#),
            Err(SpanError::NotABoolean)
        );
        assert_eq!(
            located(r#"{"settings": {"enableAutoRecovery": true, "enableAutoRecovery": false}}"#),
            Err(SpanError::DuplicateKey)
        );
        assert_eq!(
            located(r#"{"settings": {"a": 1}, "settings": {"enableAutoRecovery": true}}"#),
            Err(SpanError::DuplicateKey)
        );
    }

    #[test]
    fn rejects_malformed_and_over_deep_input() {
        assert_eq!(located("{\"settings\": {"), Err(SpanError::Malformed));
        assert_eq!(located(""), Err(SpanError::Malformed));
        let deep = format!(
            "{{\"settings\": {}{}}}",
            "[".repeat(MAX_DEPTH + 2),
            "]".repeat(MAX_DEPTH + 2)
        );
        assert_eq!(located(&deep), Err(SpanError::Malformed));
    }
}
