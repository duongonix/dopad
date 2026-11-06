use std::path::Path;

const MAX_SNIFF_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    #[default]
    PlainText,
    Rust,
    Json,
    Markdown,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Toml,
    Yaml,
    Bash,
    C,
    Cpp,
    Go,
    Java,
    Kotlin,
    Swift,
    Ruby,
    Html,
    Css,
    Scss,
    CSharp,
    Php,
    Sql,
    Xml,
    PowerShell,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommentMarkers {
    pub line: Option<&'static str>,
    pub block: Option<(&'static str, &'static str)>,
}

impl Language {
    pub const fn gpui_name(self) -> Option<&'static str> {
        match self {
            Self::PlainText => None,
            Self::Rust => Some("rust"),
            Self::Json => Some("json"),
            Self::Markdown => Some("markdown"),
            Self::Python => Some("python"),
            Self::JavaScript => Some("javascript"),
            Self::TypeScript => Some("typescript"),
            Self::Tsx => Some("tsx"),
            Self::Toml => Some("toml"),
            Self::Yaml => Some("yaml"),
            Self::Bash => Some("bash"),
            Self::C => Some("c"),
            Self::Cpp => Some("cpp"),
            Self::Go => Some("go"),
            Self::Java => Some("java"),
            Self::Kotlin => Some("kotlin"),
            Self::Swift => Some("swift"),
            Self::Ruby => Some("ruby"),
            Self::Html => Some("html"),
            Self::Css => Some("css"),
            Self::Scss => Some("scss"),
            Self::CSharp => Some("csharp"),
            Self::Php => Some("php"),
            Self::Sql => Some("sql"),
            Self::Xml => Some("xml"),
            Self::PowerShell => Some("powershell"),
        }
    }

    pub const fn comment_markers(self) -> Option<CommentMarkers> {
        match self {
            Self::PlainText => None,
            Self::Python | Self::Yaml | Self::Bash | Self::Ruby | Self::Toml | Self::PowerShell => {
                Some(CommentMarkers {
                    line: Some("#"),
                    block: None,
                })
            }
            Self::Html | Self::Markdown | Self::Xml => Some(CommentMarkers {
                line: None,
                block: Some(("<!--", "-->")),
            }),
            Self::Css | Self::Scss => Some(CommentMarkers {
                line: None,
                block: Some(("/*", "*/")),
            }),
            Self::Sql => Some(CommentMarkers {
                line: Some("--"),
                block: Some(("/*", "*/")),
            }),
            _ => Some(CommentMarkers {
                line: Some("//"),
                block: Some(("/*", "*/")),
            }),
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::PlainText => "Plain Text",
            Self::Rust => "Rust",
            Self::Json => "JSON",
            Self::Markdown => "Markdown",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Tsx => "TSX",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
            Self::Bash => "Bash",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Go => "Go",
            Self::Java => "Java",
            Self::Kotlin => "Kotlin",
            Self::Swift => "Swift",
            Self::Ruby => "Ruby",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Scss => "SCSS",
            Self::CSharp => "C#",
            Self::Php => "PHP",
            Self::Sql => "SQL",
            Self::Xml => "XML",
            Self::PowerShell => "PowerShell",
        }
    }

    pub const fn choices() -> &'static [Self] {
        &[
            Self::PlainText,
            Self::Rust,
            Self::Json,
            Self::Markdown,
            Self::Python,
            Self::JavaScript,
            Self::TypeScript,
            Self::Tsx,
            Self::Toml,
            Self::Yaml,
            Self::Bash,
            Self::C,
            Self::Cpp,
            Self::Go,
            Self::Java,
            Self::Kotlin,
            Self::Swift,
            Self::Ruby,
            Self::Html,
            Self::Css,
            Self::Scss,
            Self::CSharp,
            Self::Php,
            Self::Sql,
            Self::Xml,
            Self::PowerShell,
        ]
    }
}

pub fn detect_language(path: Option<&Path>, content_prefix: &str) -> Language {
    if let Some(name) = path
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
    {
        if let Some(language) = language_for_filename(name) {
            return language;
        }
        if let Some(language) = path
            .and_then(Path::extension)
            .and_then(|extension| extension.to_str())
            .and_then(language_for_extension)
        {
            return language;
        }
    }

    let prefix = &content_prefix
        [..content_prefix.floor_char_boundary(MAX_SNIFF_BYTES.min(content_prefix.len()))];
    let first_line = prefix.lines().next().unwrap_or_default().trim_start();
    if let Some(interpreter) = first_line.strip_prefix("#!") {
        let command = interpreter
            .split_whitespace()
            .filter(|part| !part.starts_with('-') && !part.contains('='))
            .filter_map(|part| Path::new(part).file_name()?.to_str())
            .find(|name| !matches!(*name, "env" | "busybox"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if command.starts_with("python") {
            return Language::Python;
        }
        if matches!(command.as_str(), "node" | "deno" | "bun") {
            return Language::JavaScript;
        }
        if matches!(command.as_str(), "pwsh" | "powershell") {
            return Language::PowerShell;
        }
        if matches!(
            command.as_str(),
            "bash" | "sh" | "zsh" | "dash" | "ksh" | "ash"
        ) {
            return Language::Bash;
        }
    }
    Language::PlainText
}

fn language_for_filename(name: &str) -> Option<Language> {
    match name.to_ascii_lowercase().as_str() {
        "cargo.toml" | "rustfmt.toml" | ".cargo-config.toml" => Some(Language::Toml),
        ".bashrc" | ".bash_profile" | ".zshrc" => Some(Language::Bash),
        "package.json" | "tsconfig.json" => Some(Language::Json),
        "go.mod" | "go.sum" => Some(Language::Go),
        "gemfile" | "rakefile" => Some(Language::Ruby),
        "podfile" => Some(Language::Ruby),
        _ => None,
    }
}

fn language_for_extension(extension: &str) -> Option<Language> {
    match extension.to_ascii_lowercase().as_str() {
        "rs" => Some(Language::Rust),
        "json" | "jsonc" => Some(Language::Json),
        "md" | "markdown" | "mdown" => Some(Language::Markdown),
        "py" | "pyi" => Some(Language::Python),
        "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
        "ts" | "mts" | "cts" => Some(Language::TypeScript),
        "tsx" => Some(Language::Tsx),
        "toml" => Some(Language::Toml),
        "yaml" | "yml" => Some(Language::Yaml),
        "sh" | "bash" | "zsh" => Some(Language::Bash),
        "c" | "h" => Some(Language::C),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some(Language::Cpp),
        "go" => Some(Language::Go),
        "java" => Some(Language::Java),
        "kt" | "kts" => Some(Language::Kotlin),
        "swift" => Some(Language::Swift),
        "rb" | "rake" | "gemspec" => Some(Language::Ruby),
        "html" | "htm" | "xhtml" => Some(Language::Html),
        "css" => Some(Language::Css),
        "scss" => Some(Language::Scss),
        "cs" | "csx" => Some(Language::CSharp),
        "php" | "phtml" => Some(Language::Php),
        "sql" => Some(Language::Sql),
        "xml" | "xsd" | "xsl" | "xslt" | "svg" => Some(Language::Xml),
        "ps1" | "psm1" | "psd1" => Some(Language::PowerShell),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_extensions_and_overrides() {
        assert_eq!(
            detect_language(Some(Path::new("src/main.RS")), ""),
            Language::Rust
        );
        assert_eq!(
            detect_language(Some(Path::new("package.json")), ""),
            Language::Json
        );
        assert_eq!(
            detect_language(Some(Path::new("README.md")), ""),
            Language::Markdown
        );
        assert_eq!(
            detect_language(Some(Path::new("main.cpp")), ""),
            Language::Cpp
        );
        assert_eq!(
            detect_language(Some(Path::new("app.swift")), ""),
            Language::Swift
        );
        assert_eq!(
            detect_language(Some(Path::new("App.cs")), ""),
            Language::CSharp
        );
        assert_eq!(
            detect_language(Some(Path::new("App.tsx")), ""),
            Language::Tsx
        );
        assert_eq!(
            detect_language(Some(Path::new("query.sql")), ""),
            Language::Sql
        );
        assert_eq!(
            detect_language(Some(Path::new("data.xml")), ""),
            Language::Xml
        );
        assert_eq!(
            detect_language(Some(Path::new("icon.svg")), ""),
            Language::Xml
        );
        assert_eq!(
            detect_language(Some(Path::new("script.ps1")), ""),
            Language::PowerShell
        );
        assert_eq!(
            detect_language(Some(Path::new("theme.scss")), ""),
            Language::Scss
        );
        assert_eq!(
            detect_language(Some(Path::new("Dockerfile")), ""),
            Language::PlainText
        );
    }

    #[test]
    fn sniffs_shebang_only_when_path_is_unknown() {
        assert_eq!(
            detect_language(
                Some(Path::new("script")),
                "#!/usr/bin/env python3\nprint(1)"
            ),
            Language::Python
        );
        assert_eq!(
            detect_language(Some(Path::new("script")), "#!/bin/bash\necho ok"),
            Language::Bash
        );
        assert_eq!(
            detect_language(Some(Path::new("script.unknown")), "#!/usr/bin/env python3"),
            Language::Python
        );
        assert_eq!(
            detect_language(
                Some(Path::new("script.unknown")),
                "#!/usr/bin/env pwsh\nWrite-Host ok"
            ),
            Language::PowerShell
        );
        assert_eq!(
            detect_language(
                Some(Path::new("script")),
                "#!/usr/bin/env -S bash -e\necho ok"
            ),
            Language::Bash
        );
        assert_eq!(
            detect_language(Some(Path::new("script")), "#!/usr/bin/env fish\necho ok"),
            Language::PlainText
        );
    }

    #[test]
    fn unknown_and_missing_paths_use_shebang_or_fall_back_to_plain_text() {
        assert_eq!(detect_language(None, "plain content"), Language::PlainText);
        assert_eq!(
            detect_language(Some(Path::new("data.unknown")), "hello"),
            Language::PlainText
        );
        assert_eq!(
            detect_language(None, "#!/usr/bin/env python3"),
            Language::Python
        );
    }

    #[test]
    fn reports_gpui_kit_languages_and_comment_syntax() {
        let languages = [
            Language::Rust,
            Language::Json,
            Language::Markdown,
            Language::Python,
            Language::JavaScript,
            Language::TypeScript,
            Language::Tsx,
            Language::Toml,
            Language::Yaml,
            Language::Bash,
            Language::C,
            Language::Cpp,
            Language::Go,
            Language::Java,
            Language::Kotlin,
            Language::Swift,
            Language::Ruby,
            Language::Html,
            Language::Css,
            Language::Scss,
            Language::CSharp,
            Language::Php,
            Language::Sql,
            Language::Xml,
            Language::PowerShell,
        ];
        assert_eq!(
            languages
                .iter()
                .filter(|language| language.gpui_name().is_some())
                .count(),
            25
        );
        assert_eq!(
            Language::Python.comment_markers(),
            Some(CommentMarkers {
                line: Some("#"),
                block: None,
            })
        );
        assert_eq!(
            Language::Html.comment_markers(),
            Some(CommentMarkers {
                line: None,
                block: Some(("<!--", "-->")),
            })
        );
        assert_eq!(
            Language::Sql.comment_markers(),
            Some(CommentMarkers {
                line: Some("--"),
                block: Some(("/*", "*/")),
            })
        );
        assert_eq!(
            Language::PowerShell.comment_markers(),
            Some(CommentMarkers {
                line: Some("#"),
                block: None,
            })
        );
        assert_eq!(Language::Xml.gpui_name(), Some("xml"));
        assert_eq!(Language::Scss.gpui_name(), Some("scss"));
    }

    #[test]
    fn language_mode_choices_are_unique_and_have_nonempty_names() {
        let choices = Language::choices();
        let names = choices
            .iter()
            .map(|language| language.display_name())
            .collect::<Vec<_>>();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), choices.len());
        assert!(names.iter().all(|name| !name.is_empty()));
    }
}
