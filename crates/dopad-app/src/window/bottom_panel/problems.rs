use std::sync::{Arc, atomic::Ordering};

mod rows;
pub(super) use rows::{ProblemRow, build_rows_from_documents};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, Sizable, Size,
    button::{Button, ButtonVariants},
    input::Input,
};
use gpui_kit::{
    Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px, rgb,
};
use lsp_types::DiagnosticSeverity;

use super::{BottomPanelView, ProblemsRequestKey, SeverityFilter, view_selector};
use crate::{theme::Palette, window::DopadWindow};

const HEADER_HEIGHT: f32 = 38.;
const FILE_ROW_HEIGHT: f32 = 28.;
const DIAGNOSTIC_ROW_HEIGHT: f32 = 49.;

pub(super) fn render(
    window: &mut DopadWindow,
    palette: Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    schedule_rows(window, cx);
    let counts = window.current_diagnostic_counts();
    let rows = window.bottom_panel.problems.cached_rows.clone();
    let rows_pending = window.bottom_panel.problems.pending_key.is_some();
    let all_collapsed = rows
        .iter()
        .filter_map(|row| match row {
            ProblemRow::File { collapsed, .. } => Some(collapsed),
            ProblemRow::Diagnostic { .. } => None,
        })
        .all(|collapsed| *collapsed);
    let parent = cx.entity().downgrade();
    let filter = window.bottom_panel.problems.filter.as_ref().map(Input::new);
    let filter = filter.map(|input| div().w(px(250.)).min_w(px(120.)).child(input));
    let severity = window.bottom_panel.problems.severity;
    let errors = Button::new("problems-filter-errors")
        .icon(IconName::CircleX)
        .child(counts.errors.to_string())
        .xsmall()
        .compact()
        .ghost()
        .text_color(rgb(if severity == SeverityFilter::Errors {
            0xB04B43
        } else {
            palette.secondary
        }))
        .tooltip("Show errors")
        .on_click(cx.listener(|this, _, _, cx| {
            this.bottom_panel.problems.severity =
                if this.bottom_panel.problems.severity == SeverityFilter::Errors {
                    SeverityFilter::All
                } else {
                    SeverityFilter::Errors
                };
            this.bottom_panel.invalidate_problems();
            cx.notify();
        }));
    let warnings = Button::new("problems-filter-warnings")
        .icon(IconName::TriangleAlert)
        .child(counts.warnings.to_string())
        .xsmall()
        .compact()
        .ghost()
        .text_color(rgb(if severity == SeverityFilter::Warnings {
            0x9A6A22
        } else {
            palette.secondary
        }))
        .tooltip("Show warnings")
        .on_click(cx.listener(|this, _, _, cx| {
            this.bottom_panel.problems.severity =
                if this.bottom_panel.problems.severity == SeverityFilter::Warnings {
                    SeverityFilter::All
                } else {
                    SeverityFilter::Warnings
                };
            this.bottom_panel.invalidate_problems();
            cx.notify();
        }));
    let collapse = Button::new("problems-collapse-all")
        .icon(if all_collapsed {
            IconName::ListRestart
        } else {
            IconName::ListCollapse
        })
        .xsmall()
        .compact()
        .ghost()
        .tooltip(if all_collapsed {
            "Expand all files"
        } else {
            "Collapse all files"
        })
        .on_click(cx.listener({
            let rows_for_collapse = rows.clone();
            move |this, _, _, cx| {
                if all_collapsed {
                    this.bottom_panel.problems.collapsed.clear();
                } else {
                    this.bottom_panel.problems.collapsed.extend(
                        rows_for_collapse.iter().filter_map(|row| match row {
                            ProblemRow::File { path, .. } => Some(path.clone()),
                            ProblemRow::Diagnostic { .. } => None,
                        }),
                    );
                }
                this.bottom_panel.invalidate_problems();
                cx.notify();
            }
        }));
    let selector = view_selector(
        parent.clone(),
        BottomPanelView::Problems,
        palette,
        "problems-view-selector",
    );
    let close = Button::new("problems-close-panel")
        .icon(IconName::X)
        .xsmall()
        .compact()
        .ghost()
        .tooltip("Close Bottom Panel")
        .on_click(cx.listener(|this, _, window, cx| this.hide_bottom_panel(window, cx)));

    let header = div()
        .w_full()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .px(px(10.))
        .bg(rgb(palette.chrome))
        .border_b_1()
        .border_color(rgb(palette.border))
        .child(selector)
        .child(errors)
        .child(warnings)
        .child(div().flex_1().min_w_0())
        .children(filter)
        .child(collapse)
        .child(close);

    let rows_for_render = rows.clone();
    let list = gpui_kit::uniform_list(
        "problems-diagnostic-rows",
        rows.len(),
        cx.processor(move |_this, range: std::ops::Range<usize>, _, cx| {
            range
                .filter_map(|index| rows_for_render.get(index).cloned())
                .map(|row| render_row(row, palette, cx))
                .collect::<Vec<_>>()
        }),
    )
    .h_full();
    let empty = rows.is_empty().then(|| {
        let has_any_diagnostic = window.current_diagnostic_counts().total > 0;
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(palette.secondary))
            .child(if rows_pending {
                "Loading problems..."
            } else if has_any_diagnostic {
                "No problems match the current filter"
            } else {
                "No problems detected"
            })
    });
    let body = if let Some(empty) = empty {
        empty.into_any_element()
    } else {
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(list)
            .into_any_element()
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(rgb(palette.canvas))
        .children([header.into_any_element(), body])
        .into_any_element()
}

fn schedule_rows(window: &mut DopadWindow, cx: &mut Context<DopadWindow>) {
    let workspace = window.current_diagnostic_workspace();
    let revision = window.diagnostics.revision_for(workspace.as_ref());
    let state = &mut window.bottom_panel.problems;
    let matches_request = |key: &ProblemsRequestKey| {
        key.revision == revision
            && key.workspace.as_ref() == workspace.as_ref()
            && key.filter == state.filter_text.trim()
            && key.severity == state.severity
            && key.collapsed == state.collapsed
    };
    if !state.needs_refresh
        && (state.cached_key.as_ref().is_some_and(matches_request)
            || state.pending_key.as_ref().is_some_and(matches_request))
    {
        return;
    }

    let key = ProblemsRequestKey {
        revision,
        workspace,
        filter: state.filter_text.trim().to_owned(),
        severity: state.severity,
        collapsed: state.collapsed.clone(),
    };
    state.needs_refresh = false;
    let cancellation = state.request_generation.clone();
    let generation = cancellation.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    state.pending_key = Some(key.clone());
    let Some(workspace) = key.workspace.as_ref() else {
        state.cached_rows = Arc::default();
        state.cached_key = Some(key);
        state.pending_key = None;
        return;
    };
    let documents = window
        .diagnostics
        .documents_for(Some(workspace))
        .cloned()
        .collect::<Vec<_>>();
    let build_key = key.clone();
    cx.spawn(async move |this, cx| {
        let rows = cx
            .background_executor()
            .spawn(async move {
                build_rows_from_documents(documents, &build_key, &cancellation, generation)
            })
            .await;
        let Some(rows) = rows else {
            return;
        };
        let _ = this.update(cx, |window, cx| {
            if current_request_key(window) != key {
                return;
            }
            let state = &mut window.bottom_panel.problems;
            state.cached_rows = rows.into();
            state.cached_key = Some(key.clone());
            state.pending_key = None;
            if window.bottom_panel.is_active(BottomPanelView::Problems) {
                cx.notify();
            }
        });
    })
    .detach();
}

fn current_request_key(window: &DopadWindow) -> ProblemsRequestKey {
    let workspace = window.current_diagnostic_workspace();
    ProblemsRequestKey {
        revision: window.diagnostics.revision_for(workspace.as_ref()),
        workspace,
        filter: window.bottom_panel.problems.filter_text.trim().to_owned(),
        severity: window.bottom_panel.problems.severity,
        collapsed: window.bottom_panel.problems.collapsed.clone(),
    }
}

fn render_row(
    row: ProblemRow,
    palette: Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    match row {
        ProblemRow::File {
            id,
            path,
            label,
            count,
            collapsed,
        } => div()
            .id(format!("problems-file-{id}"))
            .w_full()
            .h(px(FILE_ROW_HEIGHT))
            .px(px(12.))
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(palette.text))
            .hover(|style| style.bg(rgb(palette.tab_hover)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if collapsed {
                    this.bottom_panel.problems.collapsed.remove(&path);
                } else {
                    this.bottom_panel.problems.collapsed.insert(path.clone());
                }
                this.bottom_panel.invalidate_problems();
                cx.notify();
            }))
            .child(
                Icon::new(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .with_size(Size::Size(px(13.))),
            )
            .child(
                Icon::new(IconName::FileCode)
                    .with_size(Size::Size(px(14.)))
                    .text_color(rgb(palette.secondary)),
            )
            .child(div().flex_1().min_w_0().text_ellipsis().child(label))
            .child(
                div()
                    .text_color(rgb(palette.secondary))
                    .child(count.to_string()),
            )
            .into_any_element(),
        ProblemRow::Diagnostic {
            id,
            path,
            severity,
            message,
            source,
            code,
            line,
            column,
            character,
        } => {
            let (icon, color) = match severity {
                Some(DiagnosticSeverity::ERROR) => (IconName::CircleX, 0xB04B43),
                Some(DiagnosticSeverity::WARNING) => (IconName::TriangleAlert, 0x9A6A22),
                _ => (IconName::Info, palette.secondary),
            };
            let mut details = source;
            if let Some(code) = code {
                details.push_str(" (");
                details.push_str(&code);
                details.push(')');
            }
            details.push_str(&format!("    Ln {line}, Col {column}"));
            div()
                .id(format!("problems-diagnostic-{id}"))
                .w_full()
                .h(px(DIAGNOSTIC_ROW_HEIGHT))
                .pl(px(36.))
                .pr(px(12.))
                .flex()
                .items_start()
                .gap_2()
                .pt(px(5.))
                .text_size(px(12.))
                .text_color(rgb(palette.text))
                .hover(|style| style.bg(rgb(palette.tab_hover)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_problem(path.clone(), line, character, cx);
                }))
                .child(
                    Icon::new(icon)
                        .with_size(Size::Size(px(14.)))
                        .text_color(rgb(color)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().w_full().text_ellipsis().child(message))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(rgb(palette.secondary))
                                .child(details),
                        ),
                )
                .into_any_element()
        }
    }
}

