use crate::KEYWORDS;

/// Parse a single-line module declaration such as `pub(crate) mod foo;` or
/// `mod foo {`. Returns the name as written and whether it has an inline body.
pub fn parse_mod_decl(line: &str) -> Option<(String, bool)> {
    if line.starts_with("//") {
        return None;
    }
    let mut rest = line;
    if let Some(after_pub) = rest.strip_prefix("pub") {
        let after_pub = after_pub.trim_start();
        rest = if let Some(inner) = after_pub.strip_prefix('(') {
            let close = inner.find(')')?;
            inner[close + 1..].trim_start()
        } else {
            after_pub
        };
    }
    let rest = rest.strip_prefix("mod")?;
    let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();

    let len = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '#'))
        .unwrap_or(rest.len());
    let name = &rest[..len];
    if name.is_empty() {
        return None;
    }
    match rest[len..].trim_start().chars().next() {
        Some(';') => Some((name.to_string(), false)),
        Some('{') => Some((name.to_string(), true)),
        _ => None,
    }
}

/// The logical module name behind a written identifier (`r#type` -> `type`).
pub fn logical_name(written: &str) -> &str {
    written.strip_prefix("r#").unwrap_or(written)
}

/// Render a module name for a declaration, escaping keywords as raw idents.
pub fn mod_ident(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_declarations() {
        assert_eq!(parse_mod_decl("pub mod foo;"), Some(("foo".into(), false)));
        assert_eq!(parse_mod_decl("mod foo;"), Some(("foo".into(), false)));
        assert_eq!(
            parse_mod_decl("pub(crate) mod foo;"),
            Some(("foo".into(), false))
        );
        assert_eq!(
            parse_mod_decl("pub mod r#type;"),
            Some(("r#type".into(), false))
        );
        assert_eq!(
            parse_mod_decl("pub mod foo; // trailing"),
            Some(("foo".into(), false))
        );
    }

    #[test]
    fn parses_inline_modules() {
        assert_eq!(parse_mod_decl("pub mod foo {"), Some(("foo".into(), true)));
        assert_eq!(parse_mod_decl("mod foo { }"), Some(("foo".into(), true)));
    }

    #[test]
    fn rejects_non_declarations() {
        assert_eq!(parse_mod_decl("// pub mod foo;"), None);
        assert_eq!(parse_mod_decl("pub use foo::bar;"), None);
        assert_eq!(parse_mod_decl("pub modfoo;"), None);
        assert_eq!(parse_mod_decl("let mod = 3;"), None);
        assert_eq!(parse_mod_decl(""), None);
    }

    #[test]
    fn escapes_keywords() {
        assert_eq!(mod_ident("type"), "r#type");
        assert_eq!(mod_ident("foo"), "foo");
    }

    #[test]
    fn maps_raw_idents_to_logical_names() {
        assert_eq!(logical_name("r#type"), "type");
        assert_eq!(logical_name("plain"), "plain");
    }
}
