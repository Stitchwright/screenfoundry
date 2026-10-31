//! Text that comes back from a service, made safe to show.

/// The longest sentence shown from a service.
pub(crate) const MAX_SENTENCE: usize = 300;

/// Whether `c` is a control character or one that changes how text reads without showing (bidirectional
/// overrides and isolates, zero-width marks, the byte order mark): never shown, never in a file name.
pub(crate) fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// `text` for the person: no hidden characters, one line, trimmed, at most [`MAX_SENTENCE`] characters.
pub(crate) fn sentence(text: &str) -> String {
    let one_line: String = text.chars().map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).filter(|c| !hidden(*c)).collect();
    one_line.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_SENTENCE).collect()
}

/// The `error.message` sentence of a service's JSON answer, when it has one.
pub(crate) fn error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let m = sentence(v.get("error")?.get("message")?.as_str()?);
    (!m.is_empty()).then_some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sentence_has_no_hidden_characters_or_line_breaks() {
        assert_eq!(sentence("Too many\r\ndesigns\u{202E} today\u{0}."), "Too many designs today.");
        assert_eq!(sentence("  a \t b  "), "a b");
        assert_eq!(sentence(&"x".repeat(1000)).chars().count(), MAX_SENTENCE);
        assert_eq!(sentence("\u{200B}\u{FEFF}"), "");
    }

    #[test]
    fn the_message_comes_from_the_error_object_only() {
        assert_eq!(error_message(r#"{"error":{"message":"Nope."}}"#).as_deref(), Some("Nope."));
        for bad in ["", "not json", "{}", r#"{"error":"Nope."}"#, r#"{"error":{"message":42}}"#, r#"{"error":{"message":" \u202e "}}"#, "[]"] {
            assert_eq!(error_message(bad), None, "{bad}");
        }
    }
}
