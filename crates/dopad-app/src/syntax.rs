use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use gpui_kit::component::highlighter::{GrammarConfig, LanguageRegistry};

/// Installs the bundled queries and normalizes captures to the theme's small,
/// stable set of semantic token names. Parser instances are still created only
/// when an editor tab selects a language.
#[cfg(test)]
pub fn register_highlight_queries() {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        let registry = LanguageRegistry::singleton();
        register_additional_grammars(registry);

        for language in registry.languages() {
            register_language_highlights(registry, &language);
        }
    });
}

/// Register only the selected grammar's normalized query on first use.
pub fn register_highlight_queries_for(language: &str) {
    static REGISTERED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let registered = REGISTERED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut registered = registered
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !registered.insert(language.to_owned()) {
        return;
    }

    let registry = LanguageRegistry::singleton();
    register_additional_grammar(registry, language);
    register_language_highlights(registry, language);
}

#[cfg(test)]
fn register_additional_grammars(registry: &LanguageRegistry) {
    for language in ["xml", "scss", "powershell", "csharp", "swift"] {
        register_additional_grammar(registry, language);
    }
}

fn register_additional_grammar(registry: &LanguageRegistry, language: &str) {
    let config = match language {
        "xml" => GrammarConfig::new(
            "xml",
            tree_sitter_xml::LANGUAGE_XML.into(),
            Vec::new(),
            tree_sitter_xml::XML_HIGHLIGHT_QUERY,
            "",
            "",
        ),
        "scss" => GrammarConfig::new(
            "scss",
            arborium_scss::language().into(),
            Vec::new(),
            arborium_scss::HIGHLIGHTS_QUERY.as_str(),
            "",
            "",
        ),
        "powershell" => GrammarConfig::new(
            "powershell",
            tree_sitter_powershell::LANGUAGE.into(),
            Vec::new(),
            tree_sitter_powershell::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        "csharp" => GrammarConfig::new(
            "csharp",
            tree_sitter_c_sharp::LANGUAGE.into(),
            Vec::new(),
            tree_sitter_c_sharp::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        "swift" => GrammarConfig::new(
            "swift",
            tree_sitter_swift::LANGUAGE.into(),
            Vec::new(),
            tree_sitter_swift::HIGHLIGHTS_QUERY,
            tree_sitter_swift::INJECTIONS_QUERY,
            tree_sitter_swift::LOCALS_QUERY,
        ),
        _ => return,
    };
    registry.register(language, &config);
}

fn register_language_highlights(registry: &LanguageRegistry, language: &str) {
    let Some(mut config) = registry.language(language) else {
        return;
    };
    let normalized = normalize_highlights(language, &config.highlights);
    if normalized != config.highlights.as_ref() {
        config.highlights = normalized.into();
        registry.register(language, &config);
    }
}

fn normalize_highlights(language: &str, query: &str) -> String {
    if query.trim().is_empty() {
        return String::new();
    }

    let language_query = normalize_language_query(language, query);
    let query = language_query.as_str();
    let xml_family = matches!(language, "html" | "xml");
    let mut normalized = String::with_capacity(query.len() + 24);
    let mut cursor = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;
    for (index, character) in query.char_indices() {
        if in_comment {
            if character == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '"' {
            in_string = true;
            continue;
        }
        if character == ';' {
            in_comment = true;
            continue;
        }
        if character == '@' {
            let capture_start = index + character.len_utf8();
            let capture_end = query[capture_start..]
                .char_indices()
                .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '.')
                .last()
                .map_or(capture_start, |(offset, ch)| {
                    capture_start + offset + ch.len_utf8()
                });
            normalized.push_str(&query[cursor..index]);
            normalized.push('@');
            normalized.push_str(normalize_capture(
                xml_family,
                &query[capture_start..capture_end],
            ));
            cursor = capture_end;
        }
    }
    normalized.push_str(&query[cursor..]);
    // The SCSS grammar does not expose Tree-sitter's ERROR pseudo-node in its
    // static node table, so its highlight query must omit this optional capture.
    if language != "scss" {
        normalized.push_str("\n(ERROR) @hint\n");
    }
    normalized
}

fn normalize_language_query(language: &str, query: &str) -> String {
    if language == "scss" {
        return format!(
            "{query}\n(declaration (property_name) @variable (#match? @variable \"^\\\\$\"))\n"
        );
    }
    if matches!(language, "javascript" | "typescript" | "tsx") {
        return query.replace("] @constant.builtin", "] @boolean");
    }
    if language == "json" {
        return query.replace("(null) @constant.builtin", "(null) @boolean");
    }
    if language == "go" {
        return query.replace(
            "[\n  (nil)\n  (iota)\n] @constant.builtin",
            "(nil) @boolean\n(iota) @constant",
        );
    }
    if language == "zig" {
        return query.replace("] @constant.builtin", "] @boolean");
    }
    if language == "rust" {
        let query = query
            .replace(
                "(boolean_literal) @constant.builtin",
                "(boolean_literal) @boolean",
            )
            .replace(
                "(integer_literal) @constant.builtin",
                "(integer_literal) @number",
            )
            .replace(
                "(float_literal) @constant.builtin",
                "(float_literal) @number",
            );
        return format!("{query}\n(let_declaration pattern: (identifier) @variable)\n");
    }
    query.to_owned()
}

fn normalize_capture(xml_family: bool, capture: &str) -> &str {
    if capture == "variable.parameter" || capture.starts_with("variable.parameter.") {
        return "variable.special";
    }
    if capture == "variable.builtin" || capture.starts_with("variable.builtin.") {
        return "variable.special";
    }
    if capture == "variable.member" || capture.starts_with("variable.member.") {
        return "property";
    }
    if capture == "variable.global" || capture.starts_with("variable.global.") {
        return "constant";
    }
    if capture == "module" || capture.starts_with("module.") {
        return "type";
    }
    if capture == "macro" || capture.starts_with("macro.") || capture.starts_with("function.macro")
    {
        return "attribute";
    }
    if xml_family && capture == "attribute" {
        return "variant";
    }
    if capture == "invalid" || capture == "error" {
        return "hint";
    }
    capture
}

#[cfg(test)]
mod tests {
    use super::{normalize_highlights, register_highlight_queries};
    use gpui_kit::component::highlighter::LanguageRegistry;

    #[test]
    fn normalizes_semantic_capture_names_without_touching_query_structure() {
        let query =
            "(identifier) @variable.parameter\n(identifier) @function.macro\n(ERROR) @invalid";
        let normalized = normalize_highlights("rust", query);
        assert!(normalized.contains("@variable.special"));
        assert!(normalized.contains("@attribute"));
        assert!(normalized.contains("@hint"));
        assert!(normalized.contains("(ERROR) @hint"));

        let xml = normalize_highlights("xml", "(attribute) @attribute");
        assert!(xml.contains("@variant"));

        let scss = normalize_highlights("scss", "\"@error\" @keyword");
        assert!(scss.contains("\"@error\" @keyword"));
        assert!(!scss.contains("\"@hint\""));

        let javascript =
            normalize_highlights("javascript", "[\n  (true)\n  (null)\n] @constant.builtin");
        assert!(javascript.contains("] @boolean"));
        assert!(!javascript.contains("@constant.builtin"));

        let json = normalize_highlights("json", "(null) @constant.builtin");
        assert!(json.contains("(null) @boolean"));

        let go = normalize_highlights("go", "[\n  (nil)\n  (iota)\n] @constant.builtin");
        assert!(go.contains("(nil) @boolean"));
        assert!(go.contains("(iota) @constant"));

        let zig = normalize_highlights("zig", "[\n  \"null\"\n] @constant.builtin");
        assert!(zig.contains("] @boolean"));
    }

    #[test]
    fn every_registered_highlight_query_compiles_with_its_grammar() {
        register_highlight_queries();
        let registry = LanguageRegistry::singleton();
        let mut checked = 0;
        for name in registry.languages() {
            let Some(config) = registry.language(&name) else {
                continue;
            };
            let Some(grammar) = config.language.as_ref() else {
                continue;
            };
            let source = format!(
                "{}{}{}",
                config.injections, config.locals, config.highlights
            );
            if source.trim().is_empty() {
                continue;
            }
            tree_sitter::Query::new(grammar, &source)
                .unwrap_or_else(|error| panic!("invalid query for {}: {error}\n{source}", name));
            checked += 1;
        }
        assert!(
            checked >= 20,
            "expected broad grammar coverage, checked {checked}"
        );
    }

    #[test]
    fn scss_registry_parses_current_module_and_mixin_syntax() {
        use gpui_kit::component::highlighter::LanguageRegistry;

        register_highlight_queries();
        let source = "$gap: 1rem;\n@use 'tokens';\n@mixin surface($tone) { color: $tone; }\n.card { @include surface(#fff); }";
        let config = LanguageRegistry::singleton()
            .language("scss")
            .expect("SCSS grammar was registered");
        let grammar = config
            .language
            .as_ref()
            .expect("SCSS grammar has a Tree-sitter language");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(grammar)
            .expect("SCSS grammar matches the app's Tree-sitter version");
        let tree = parser.parse(source, None).expect("SCSS parse completes");
        assert!(!tree.root_node().has_error(), "current SCSS syntax parses");
    }
}
