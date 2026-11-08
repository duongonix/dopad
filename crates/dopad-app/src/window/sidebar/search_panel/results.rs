use std::path::Path;

use gpui_kit::component::{
    Icon, Sizable, Size,
    button::{Button, ButtonVariants},
};
use gpui_kit::{
    Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, px, rgb,
};

use super::super::super::{DopadWindow, file_search};
use super::runner::SearchNavigation;
use crate::theme::Palette;

#[derive(Clone, Copy)]
enum SearchRow {
    File(usize),
    Match(usize, usize),
}

#[derive(Clone, Copy)]
struct ResultColors {
    text: u32,
    secondary: u32,
    muted: u32,
    accent: u32,
    tab_hover: u32,
}

pub(super) fn render_results(
    window: &DopadWindow,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let search = &window.sidebar.search;
    if search.results.is_empty() {
        return div().into_any_element();
    }

    let filter = search
        .filter
        .as_ref()
        .map(|filter| filter.read(cx).value().to_string().to_lowercase())
        .unwrap_or_default();
    let mut rows = Vec::new();
    for (file_index, result) in search.results.iter().enumerate() {
        if !filter.is_empty()
            && !result
                .path
                .to_string_lossy()
                .to_lowercase()
                .contains(&filter)
        {
            continue;
        }
        rows.push(SearchRow::File(file_index));
        if !search.collapsed.contains(&result.path) {
            rows.extend(
                (0..result.lines.len()).map(|line_index| SearchRow::Match(file_index, line_index)),
            );
        }
    }

    let mut content = div().w_full().flex_1().min_h_0().flex().flex_col().gap_1();
    content = content.child(
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div().flex_1().min_w_0().child(
                    search
                        .filter
                        .as_ref()
                        .map(gpui_kit::component::input::Input::new)
                        .expect("filter input initialized"),
                ),
            )
            .child(
                Button::new("search-collapse-all")
                    .icon(gpui_kit::assets::IconName::ListCollapse)
                    .xsmall()
                    .compact()
                    .ghost()
                    .tooltip("Collapse all results")
                    .on_click(cx.listener(|this, _, _, cx| {
                        for result in &this.sidebar.search.results {
                            this.sidebar.search.collapsed.insert(result.path.clone());
                        }
                        cx.notify();
                    })),
            ),
    );

    if let Some(summary) = search.summary.as_ref() {
        let mut text = format!(
            "{} results in {} files ({:.0} ms)",
            summary.match_count,
            summary.files.len(),
            summary.elapsed.as_secs_f64() * 1000.0
        );
        if summary.truncated {
            text.push_str("; result limit reached");
        }
        if summary.skipped_files > 0 {
            text.push_str(&format!(
                "; skipped {} unreadable/binary files",
                summary.skipped_files
            ));
        }
        if summary.skipped_long_lines > 0 {
            text.push_str(&format!(
                "; skipped {} lines over 1 MiB",
                summary.skipped_long_lines
            ));
        }
        content = content.child(
            div()
                .text_xs()
                .text_color(rgb(palette.secondary))
                .child(text),
        );
    }

    let colors = ResultColors {
        text: palette.text,
        secondary: palette.secondary,
        muted: palette.muted,
        accent: palette.accent,
        tab_hover: palette.tab_hover,
    };
    let list = gpui_kit::uniform_list(
        "search-result-rows",
        rows.len(),
        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
            range
                .filter_map(|index| match *rows.get(index)? {
                    SearchRow::File(file_index) => {
                        let result = this.sidebar.search.results.get(file_index)?;
                        Some(render_file_row(
                            result,
                            this.sidebar.search.collapsed.contains(&result.path),
                            colors,
                            cx,
                        ))
                    }
                    SearchRow::Match(file_index, line_index) => {
                        let result = this.sidebar.search.results.get(file_index)?;
                        let line = result.lines.get(line_index)?;
                        Some(render_match_line(result, line, colors, cx))
                    }
                })
                .collect::<Vec<_>>()
        }),
    )
    .h_full();

    content
        .child(div().flex_1().min_h_0().w_full().child(list))
        .into_any_element()
}

fn render_file_row(
    result: &file_search::FileMatches,
    collapsed: bool,
    colors: ResultColors,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let path = result.path.clone();
    div()
        .id(format!("search-result-{}", stable_path_id(&path)))
        .w_full()
        .h(px(27.))
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(rgb(colors.text))
        .child(
            Icon::new(if collapsed {
                gpui_kit::assets::IconName::ChevronRight
            } else {
                gpui_kit::assets::IconName::ChevronDown
            })
            .with_size(Size::Size(px(12.)))
            .text_color(rgb(colors.secondary)),
        )
        .child(
            Icon::new(gpui_kit::assets::IconName::FileText)
                .with_size(Size::Size(px(14.)))
                .text_color(rgb(colors.accent)),
        )
        .child(div().flex_1().min_w_0().text_ellipsis().child(format!(
            "{} ({})",
            result.display_name,
            result.lines.len()
        )))
        .on_click(cx.listener(move |this, _, _, cx| {
            if !this.sidebar.search.collapsed.insert(path.clone()) {
                this.sidebar.search.collapsed.remove(&path);
            }
            cx.notify();
        }))
        .into_any_element()
}

fn render_match_line(
    result: &file_search::FileMatches,
    line: &file_search::MatchLine,
    colors: ResultColors,
    cx: &mut Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let snippet = line.snippet.as_str();
    let start = line.match_range.start.min(snippet.len());
    let end = line.match_range.end.min(snippet.len()).max(start);
    let before = snippet[..start].to_owned();
    let matched = snippet[start..end].to_owned();
    let after = snippet[end..].to_owned();
    let navigation = SearchNavigation {
        path: result.path.clone(),
        document_id: result.document_id,
        revision: result.revision,
        disk_identity: result.disk_identity,
        position: dopad_core::TextPosition::new(line.line.saturating_sub(1), line.column),
        match_columns: line.match_columns,
    };
    let path = result.path.clone();
    div()
        .id(format!(
            "search-match-{}-{}",
            stable_path_id(&path),
            line.line
        ))
        .w_full()
        .h(px(27.))
        .flex()
        .items_center()
        .gap_2()
        .pl(px(20.))
        .text_xs()
        .text_color(rgb(colors.secondary))
        .hover(|style| style.bg(rgb(colors.tab_hover)))
        .child(
            div()
                .w(px(32.))
                .flex_shrink_0()
                .text_color(rgb(colors.muted))
                .child(line.line.to_string()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(before)
                .child(
                    div()
                        .text_color(rgb(colors.accent))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child(matched),
                )
                .child(after),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.navigate_to_search_result(
                SearchNavigation {
                    path: path.clone(),
                    document_id: navigation.document_id,
                    revision: navigation.revision,
                    disk_identity: navigation.disk_identity,
                    position: navigation.position,
                    match_columns: navigation.match_columns,
                },
                cx,
            );
        }))
        .into_any_element()
}

fn stable_path_id(path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
