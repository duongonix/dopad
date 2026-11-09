use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use super::super::{ProblemsRequestKey, SeverityFilter};
use crate::window::diagnostics::StoredDiagnostics;

#[cfg(test)]
use std::collections::HashSet;

#[cfg(test)]
use crate::window::diagnostics::DiagnosticStore;

#[derive(Clone)]
pub(in crate::window::bottom_panel) enum ProblemRow {
    File {
        id: u64,
        path: PathBuf,
        label: String,
        count: usize,
        collapsed: bool,
    },
    Diagnostic {
        id: u64,
        path: PathBuf,
        severity: Option<DiagnosticSeverity>,
        message: String,
        source: String,
        code: Option<String>,
        line: usize,
        column: usize,
        character: usize,
    },
}

pub(in crate::window::bottom_panel) fn build_rows_from_documents(
    documents: Vec<StoredDiagnostics>,
    request: &ProblemsRequestKey,
    cancellation: &Arc<AtomicU64>,
    generation: u64,
) -> Option<Vec<ProblemRow>> {
    if cancellation.load(Ordering::Relaxed) != generation {
        return None;
    }
    let query = request.filter.trim().to_lowercase();
    let mut files: BTreeMap<PathBuf, Vec<DiagnosticRow>> = BTreeMap::new();
    for document in documents {
        if !collect_document_rows(
            &document,
            request.severity,
            &query,
            &mut files,
            cancellation,
            generation,
        ) {
            return None;
        }
    }
    let mut rows = Vec::new();
    for (path, mut diagnostics) in files {
        if cancellation.load(Ordering::Relaxed) != generation {
            return None;
        }
        diagnostics.sort_by_key(|diagnostic| (diagnostic.line, diagnostic.column));
        let is_collapsed = request.collapsed.contains(&path);
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let count = diagnostics.len();
        rows.push(ProblemRow::File {
            id: stable_id(&path, "file", 0),
            path: path.clone(),
            label,
            count,
            collapsed: is_collapsed,
        });
        if !is_collapsed {
            rows.extend(
                diagnostics
                    .into_iter()
                    .map(|diagnostic| ProblemRow::Diagnostic {
                        id: diagnostic.id,
                        path: path.clone(),
                        severity: diagnostic.severity,
                        message: diagnostic.message,
                        source: diagnostic.source,
                        code: diagnostic.code,
                        line: diagnostic.line,
                        column: diagnostic.column,
                        character: diagnostic.character,
                    }),
            );
        }
    }
    Some(rows)
}

struct DiagnosticRow {
    id: u64,
    severity: Option<DiagnosticSeverity>,
    message: String,
    source: String,
    code: Option<String>,
    line: usize,
    column: usize,
    character: usize,
}

fn collect_document_rows(
    document: &StoredDiagnostics,
    severity_filter: SeverityFilter,
    query: &str,
    files: &mut BTreeMap<PathBuf, Vec<DiagnosticRow>>,
    cancellation: &AtomicU64,
    generation: u64,
) -> bool {
    for (index, diagnostic) in document.diagnostics.iter().enumerate() {
        if cancellation.load(Ordering::Relaxed) != generation {
            return false;
        }
        if !severity_matches(severity_filter, diagnostic.severity) {
            continue;
        }
        let source = diagnostic
            .source
            .clone()
            .unwrap_or_else(|| document.server_id.clone());
        let code = diagnostic.code.as_ref().map(diagnostic_code);
        if !matches_query(query, &document.path, diagnostic, &source, code.as_deref()) {
            continue;
        }
        let line = usize::try_from(diagnostic.range.start.line)
            .unwrap_or(usize::MAX)
            .saturating_add(1);
        let character = usize::try_from(diagnostic.range.start.character).unwrap_or(usize::MAX);
        let column = character.saturating_add(1);
        let id = stable_id(&document.path, &diagnostic.message, index as u64);
        files
            .entry(document.path.clone())
            .or_default()
            .push(DiagnosticRow {
                id,
                severity: diagnostic.severity,
                message: diagnostic.message.clone(),
                source,
                code,
                line,
                column,
                character,
            });
    }
    true
}

fn severity_matches(filter: SeverityFilter, severity: Option<DiagnosticSeverity>) -> bool {
    match filter {
        SeverityFilter::All => true,
        SeverityFilter::Errors => severity == Some(DiagnosticSeverity::ERROR),
        SeverityFilter::Warnings => severity == Some(DiagnosticSeverity::WARNING),
    }
}

fn matches_query(
    query: &str,
    path: &std::path::Path,
    diagnostic: &Diagnostic,
    source: &str,
    code: Option<&str>,
) -> bool {
    query.is_empty()
        || path.to_string_lossy().to_lowercase().contains(query)
        || diagnostic.message.to_lowercase().contains(query)
        || source.to_lowercase().contains(query)
        || code.is_some_and(|code| code.to_lowercase().contains(query))
}

fn diagnostic_code(code: &NumberOrString) -> String {
    match code {
        NumberOrString::Number(value) => value.to_string(),
        NumberOrString::String(value) => value.clone(),
    }
}

fn stable_id(path: &std::path::Path, content: &str, index: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    content.hash(&mut hasher);
    index.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
fn build_rows(
    store: &DiagnosticStore,
    workspace: Option<&PathBuf>,
    filter: &str,
    severity: SeverityFilter,
    collapsed: &HashSet<PathBuf>,
) -> Vec<ProblemRow> {
    let Some(workspace) = workspace else {
        return Vec::new();
    };
    let key = ProblemsRequestKey {
        revision: store.revision_for(Some(workspace)),
        workspace: Some(workspace.clone()),
        filter: filter.trim().to_owned(),
        severity,
        collapsed: collapsed.clone(),
    };
    let documents = store.documents_for(Some(workspace)).cloned().collect();
    build_rows_from_documents(documents, &key, &Arc::new(AtomicU64::new(0)), 0).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

    use super::{ProblemRow, build_rows};
    use crate::window::{
        bottom_panel::SeverityFilter,
        diagnostics::{DiagnosticStore, DiagnosticUpdate},
    };

    fn diagnostic(severity: DiagnosticSeverity, message: &str, source: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(2, 4), Position::new(2, 6)),
            severity: Some(severity),
            code: Some(NumberOrString::String(format!("{source}-code"))),
            source: Some(source.into()),
            message: message.into(),
            ..Diagnostic::default()
        }
    }

    #[test]
    fn rows_group_by_file_and_filter_message_source_and_severity() {
        let workspace = std::env::temp_dir().join("dopad-problems-ui-test");
        let path = workspace.join("src/main.rs");
        let url = url::Url::from_file_path(&path).expect("valid file URL");
        let mut store = DiagnosticStore::default();
        store.apply(
            DiagnosticUpdate {
                workspace: workspace.clone(),
                server_id: "rust-analyzer".into(),
                params: lsp_types::PublishDiagnosticsParams {
                    uri: url.as_str().parse().expect("valid LSP URI"),
                    diagnostics: vec![
                        diagnostic(DiagnosticSeverity::ERROR, "missing type", "rust-analyzer"),
                        diagnostic(DiagnosticSeverity::WARNING, "unused value", "clippy"),
                    ],
                    version: Some(1),
                },
            },
            Some(1),
        );
        let rows = build_rows(
            &store,
            Some(&workspace),
            "clippy",
            SeverityFilter::All,
            &HashSet::new(),
        );
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0], ProblemRow::File { count: 1, .. }));
        assert!(matches!(
            rows[1],
            ProblemRow::Diagnostic {
                line: 3,
                column: 5,
                ..
            }
        ));
        let code_matches = build_rows(
            &store,
            Some(&workspace),
            "clippy-code",
            SeverityFilter::All,
            &HashSet::new(),
        );
        assert_eq!(code_matches.len(), 2);
        let message_matches = build_rows(
            &store,
            Some(&workspace),
            "missing type",
            SeverityFilter::All,
            &HashSet::new(),
        );
        assert_eq!(message_matches.len(), 2);
        let filename_matches = build_rows(
            &store,
            Some(&workspace),
            "main.rs",
            SeverityFilter::All,
            &HashSet::new(),
        );
        assert_eq!(filename_matches.len(), 3);
        let errors = build_rows(
            &store,
            Some(&workspace),
            "",
            SeverityFilter::Errors,
            &HashSet::new(),
        );
        assert_eq!(errors.len(), 2);
        assert!(matches!(
            errors[1],
            ProblemRow::Diagnostic {
                severity: Some(DiagnosticSeverity::ERROR),
                ..
            }
        ));
    }
}
