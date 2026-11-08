use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use dopad_syntax::Language;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum InstallMethod {
    Rustup,
    Npm,
    PythonPip,
    GoInstall,
    Manual,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ServerSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub language: &'static str,
    pub executable: &'static str,
    pub args: &'static [&'static str],
    pub version_args: &'static [&'static str],
    pub method: InstallMethod,
    pub package: &'static str,
    pub manual_url: &'static str,
}

pub(super) struct InstallPlan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub expected_executable: Option<PathBuf>,
}

const RUST: ServerSpec = ServerSpec {
    id: "rust-analyzer",
    name: "rust-analyzer",
    language: "Rust",
    executable: "rust-analyzer",
    args: &[],
    version_args: &["--version"],
    method: InstallMethod::Rustup,
    package: "rust-analyzer",
    manual_url: "https://rust-analyzer.github.io/book/installation.html",
};

const TYPESCRIPT: ServerSpec = ServerSpec {
    id: "typescript-language-server",
    name: "TypeScript Language Server",
    language: "TypeScript / JavaScript",
    executable: "typescript-language-server",
    args: &["--stdio"],
    version_args: &["--version"],
    method: InstallMethod::Npm,
    package: "typescript typescript-language-server",
    manual_url: "https://github.com/typescript-language-server/typescript-language-server",
};

const PYTHON: ServerSpec = ServerSpec {
    id: "basedpyright",
    name: "Basedpyright",
    language: "Python",
    executable: "basedpyright-langserver",
    args: &["--stdio"],
    version_args: &["--version"],
    method: InstallMethod::PythonPip,
    package: "basedpyright",
    manual_url: "https://docs.basedpyright.com/latest/installation/",
};

const C_LANG: ServerSpec = ServerSpec {
    id: "clangd",
    name: "clangd",
    language: "C / C++",
    executable: "clangd",
    args: &["--background-index"],
    version_args: &["--version"],
    method: InstallMethod::Manual,
    package: "",
    manual_url: "https://clangd.llvm.org/installation.html",
};

const GO: ServerSpec = ServerSpec {
    id: "gopls",
    name: "gopls",
    language: "Go",
    executable: "gopls",
    args: &[],
    version_args: &["version"],
    method: InstallMethod::GoInstall,
    package: "golang.org/x/tools/gopls@latest",
    manual_url: "https://go.dev/gopls/",
};

const HTML: ServerSpec = ServerSpec {
    id: "vscode-html-language-server",
    name: "VS Code HTML Language Server",
    language: "HTML",
    executable: "vscode-html-language-server",
    args: &["--stdio"],
    version_args: &["--version"],
    method: InstallMethod::Npm,
    package: "vscode-langservers-extracted",
    manual_url: "https://github.com/hrsh7th/vscode-langservers-extracted",
};

const CSS: ServerSpec = ServerSpec {
    id: "vscode-css-language-server",
    name: "VS Code CSS Language Server",
    language: "CSS",
    executable: "vscode-css-language-server",
    args: &["--stdio"],
    version_args: &["--version"],
    method: InstallMethod::Npm,
    package: "vscode-langservers-extracted",
    manual_url: "https://github.com/hrsh7th/vscode-langservers-extracted",
};

const JSON: ServerSpec = ServerSpec {
    id: "vscode-json-language-server",
    name: "VS Code JSON Language Server",
    language: "JSON",
    executable: "vscode-json-language-server",
    args: &["--stdio"],
    version_args: &["--version"],
    method: InstallMethod::Npm,
    package: "vscode-langservers-extracted",
    manual_url: "https://github.com/hrsh7th/vscode-langservers-extracted",
};

const LUA: ServerSpec = ServerSpec {
    id: "lua-language-server",
    name: "Lua Language Server",
    language: "Lua",
    executable: "lua-language-server",
    args: &[],
    version_args: &["--version"],
    method: InstallMethod::Manual,
    package: "",
    manual_url: "https://luals.github.io/#install",
};

pub(super) fn for_language(language: Language) -> Option<&'static ServerSpec> {
    match language {
        Language::Rust => Some(&RUST),
        Language::JavaScript | Language::TypeScript | Language::Tsx => Some(&TYPESCRIPT),
        Language::Python => Some(&PYTHON),
        Language::C | Language::Cpp => Some(&C_LANG),
        Language::Go => Some(&GO),
        Language::Html => Some(&HTML),
        Language::Css | Language::Scss => Some(&CSS),
        Language::Json => Some(&JSON),
        _ => None,
    }
}

pub(super) fn for_file(language: Language, path: &std::path::Path) -> Option<&'static ServerSpec> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lua"))
    {
        return Some(&LUA);
    }
    for_language(language)
}

pub(super) fn find_executable(name: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(name);
    if direct.components().count() > 1 && direct.is_file() {
        return Some(direct);
    }

    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|directory| {
            extensions
                .iter()
                .map(move |extension| directory.join(format!("{name}{extension}")))
        })
        .find(|candidate| candidate.is_file())
}

pub(super) fn install_command(spec: &ServerSpec, data_dir: &Path) -> Option<InstallPlan> {
    let mut env = Vec::new();
    let install_root = data_dir.join("lsp");
    let (program, args, expected_executable): (&str, Vec<OsString>, Option<PathBuf>) =
        match spec.method {
            InstallMethod::Rustup => (
                "rustup",
                strings(["component", "add", "rust-analyzer"]),
                None,
            ),
            InstallMethod::Npm => {
                let prefix = install_root.join("npm");
                let mut args = vec![
                    "install".into(),
                    "--prefix".into(),
                    prefix.as_os_str().to_owned(),
                    "--no-audit".into(),
                    "--no-fund".into(),
                    "--progress=false".into(),
                ];
                args.extend(spec.package.split_whitespace().map(OsString::from));
                let binary = prefix
                    .join("node_modules")
                    .join(".bin")
                    .join(if cfg!(windows) {
                        format!("{}.cmd", spec.executable)
                    } else {
                        spec.executable.to_owned()
                    });
                ("npm", args, Some(binary))
            }
            InstallMethod::PythonPip => (
                "python",
                strings(["-m", "pip", "install", "--user", "basedpyright"]),
                None,
            ),
            InstallMethod::GoInstall => {
                let bin = install_root.join("bin");
                env.push((OsString::from("GOBIN"), bin.clone().into_os_string()));
                let executable = bin.join(if cfg!(windows) {
                    format!("{}.exe", spec.executable)
                } else {
                    spec.executable.to_owned()
                });
                (
                    "go",
                    strings(["install", "golang.org/x/tools/gopls@latest"]),
                    Some(executable),
                )
            }
            InstallMethod::Manual => return None,
        };
    let executable = find_executable(program).or_else(|| {
        (program == "python")
            .then(|| find_executable("py"))
            .flatten()
    })?;
    Some(InstallPlan {
        program: executable,
        args,
        env,
        expected_executable,
    })
}

fn strings<const N: usize>(values: [&str; N]) -> Vec<OsString> {
    values.into_iter().map(OsString::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_only_languages_with_registered_servers() {
        assert_eq!(
            for_language(Language::Rust).map(|s| s.id),
            Some("rust-analyzer")
        );
        assert_eq!(
            for_language(Language::TypeScript).map(|s| s.id),
            Some("typescript-language-server")
        );
        assert_eq!(
            for_language(Language::Html).map(|s| s.executable),
            Some("vscode-html-language-server")
        );
        assert_eq!(
            for_language(Language::Css).map(|s| s.executable),
            Some("vscode-css-language-server")
        );
        assert_eq!(
            for_language(Language::Json).map(|s| s.executable),
            Some("vscode-json-language-server")
        );
        assert!(for_language(Language::Markdown).is_none());
        assert!(for_language(Language::PlainText).is_none());
        assert_eq!(
            for_file(Language::PlainText, std::path::Path::new("script.lua")).map(|s| s.id),
            Some("lua-language-server")
        );
    }

    #[test]
    fn manual_servers_have_no_automatic_install_command() {
        assert!(install_command(&C_LANG, Path::new("/data")).is_none());
        assert!(install_command(&LUA, Path::new("/data")).is_none());
    }
}
