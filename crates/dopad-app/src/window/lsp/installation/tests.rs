use super::{ProcessOutput, npm_failure_message, resolve_npm_entrypoint};
use crate::window::lsp::registry;
use dopad_syntax::Language;
use std::{fs, path::Path};

#[test]
fn npm_registry_failures_name_registry_network_instead_of_system_node() {
    let output = ProcessOutput {
        success: false,
        stdout: String::new(),
        stderr: "FetchError request to https://registry.npmjs.org failed: ECONNRESET".to_owned(),
    };

    let message = npm_failure_message("TypeScript Language Server", &output);

    assert!(message.contains("internet connection or proxy"));
    assert!(!message.contains("FetchError"));
    assert!(!message.contains("registry.npmjs.org"));
    assert!(!message.contains("Check that Node.js"));
}

#[test]
fn npm_permission_failures_do_not_show_raw_installer_output() {
    let output = ProcessOutput {
        success: false,
        stdout: String::new(),
        stderr: "npm ERR! EACCES: permission denied".to_owned(),
    };

    let message = npm_failure_message("TypeScript Language Server", &output);

    assert!(message.contains("app data directory"));
    assert!(!message.contains("npm ERR!"));
}

#[test]
fn resolves_typescript_server_entrypoint_from_pinned_package_manifest() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let prefix = temporary.path();
    let server_root = prefix.join("node_modules/typescript-language-server");
    let entrypoint = server_root.join("lib/cli.mjs");
    fs::create_dir_all(entrypoint.parent().expect("entrypoint parent"))
        .expect("create package directory");
    fs::write(entrypoint, "export {}; ").expect("write server entrypoint");
    fs::create_dir_all(prefix.join("node_modules/typescript")).expect("create TypeScript package");
    fs::write(
        prefix.join("node_modules/typescript/package.json"),
        r#"{"name":"typescript","version":"6.0.3"}"#,
    )
    .expect("write TypeScript manifest");
    fs::write(
        server_root.join("package.json"),
        r#"{"name":"typescript-language-server","bin":{"typescript-language-server":"lib/cli.mjs"}}"#,
    )
    .expect("write server manifest");

    let spec = registry::for_language(Language::TypeScript).expect("TypeScript server spec");
    let resolved = resolve_npm_entrypoint(prefix, spec).expect("resolve server entrypoint");
    assert!(resolved.ends_with(Path::new("lib/cli.mjs")));
}

#[test]
fn rejects_npm_entrypoints_that_escape_the_package_root() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let prefix = temporary.path();
    let server_root = prefix.join("node_modules/typescript-language-server");
    fs::create_dir_all(&server_root).expect("create package directory");
    fs::write(
        server_root.join("package.json"),
        r#"{"bin":{"typescript-language-server":"../../outside.js"}}"#,
    )
    .expect("write unsafe manifest");
    let spec = registry::for_language(Language::TypeScript).expect("TypeScript server spec");
    assert!(resolve_npm_entrypoint(prefix, spec).is_err());
}
