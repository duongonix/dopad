use gpui_kit::component::input::{
    Copy as EditorCopy, Cut as EditorCut, Editor as KitEditor, Paste as EditorPaste,
    Redo as EditorRedo, SelectAll as EditorSelectAll, Undo as EditorUndo,
};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::{
    Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px, relative, rgb,
};

use crate::theme::Palette;
use crate::window::{
    DopadWindow, file_view::TabContent, image_preview, preview_view::render_preview,
};

#[derive(Clone)]
pub(super) struct BodyState {
    pub preview_mode: bool,
    pub font_size: f32,
    pub font_weight: u16,
    pub line_height: f32,
    pub font_family: String,
}

pub(super) struct BodyView {
    pub element: gpui_kit::AnyElement,
    pub state: BodyState,
}

pub(super) fn render(
    window: &mut DopadWindow,
    ui_window: &mut Window,
    palette: &Palette,
    cx: &mut Context<DopadWindow>,
) -> BodyView {
    let settings = window.settings.current();
    let (active_id, state) = window.active_tab().map_or(
        (
            None,
            BodyState {
                preview_mode: false,
                font_size: f32::from(settings.editor_font_size),
                font_weight: settings.editor_font_weight,
                line_height: f32::from(settings.editor_line_height) / 100.0,
                font_family: settings.editor_font_family.clone(),
            },
        ),
        |tab| {
            (
                Some(tab.id),
                BodyState {
                    preview_mode: tab.preview_mode
                        && crate::window::preview::supports_tab_preview(tab),
                    font_size: tab.font_size,
                    font_weight: settings.editor_font_weight,
                    line_height: f32::from(settings.editor_line_height) / 100.0,
                    font_family: settings.editor_font_family.clone(),
                },
            )
        },
    );

    let active_content = window.active_tab().map(|tab| {
        (
            tab.title.clone(),
            tab.content.clone(),
            tab.loading_indicator_visible,
        )
    });
    let element = match active_content {
        Some((_, TabContent::Image(_), _)) => image_preview::render(window, ui_window, palette, cx),
        Some((
            title,
            content @ (TabContent::Loading | TabContent::Unsupported(_) | TabContent::Error(_)),
            loading_visible,
        )) => super::super::unsupported_file::render_file_state(
            &title,
            &content,
            loading_visible,
            palette,
            cx,
        ),
        _ if state.preview_mode => {
            render_preview_body(window, active_id, state.font_size, palette, ui_window)
        }
        _ => render_editor_body(window, &state, palette, cx),
    };

    BodyView { element, state }
}

fn render_editor_body(
    window: &DopadWindow,
    view_state: &BodyState,
    palette: &Palette,
    cx: &Context<DopadWindow>,
) -> gpui_kit::AnyElement {
    let Some(state) = window.active_tab().and_then(|tab| tab.ui.as_ref()) else {
        return div().size_full().bg(rgb(palette.canvas)).into_any_element();
    };

    let mut editor = KitEditor::new(state)
        .flex_1()
        .min_h_0()
        .w_full()
        .appearance(false)
        .bordered(false)
        .text_size(px(view_state.font_size))
        .line_height(relative(view_state.line_height));
    if view_state.font_weight != 400 {
        editor = editor.font_weight(match view_state.font_weight {
            500 => gpui_kit::FontWeight::MEDIUM,
            600 => gpui_kit::FontWeight::SEMIBOLD,
            700 => gpui_kit::FontWeight::BOLD,
            800 => gpui_kit::FontWeight::EXTRA_BOLD,
            _ => gpui_kit::FontWeight::NORMAL,
        });
    }
    if view_state.font_family != "System Monospace" {
        editor = editor.font_family(view_state.font_family.clone());
    }

    let active_id = window.active_tab().map(|tab| tab.id);
    let format_available = window.active_tab().is_some_and(|tab| {
        tab.editor.is_some()
            && tab
                .path
                .as_deref()
                .is_some_and(crate::window::formatting::supports_path)
    });
    let can_undo = window
        .active_tab()
        .and_then(|tab| tab.editor.as_ref())
        .is_some_and(dopad_editor::Editor::can_undo);
    let can_redo = window
        .active_tab()
        .and_then(|tab| tab.editor.as_ref())
        .is_some_and(dopad_editor::Editor::can_redo);
    let context_state = state.clone();
    let editor_area = div()
        .id(format!(
            "editor-context-menu-area-{}",
            active_id.unwrap_or_default()
        ))
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .child(editor)
        .context_menu(move |menu, _, cx| {
            let has_clipboard_text = cx
                .read_from_clipboard()
                .is_some_and(|clipboard| clipboard.text().is_some());
            let (has_selection, has_text) = {
                let state = context_state.read(cx);
                let selection = state.selected_range();
                (
                    selection.start < selection.end,
                    state.text().len_chars() > 0,
                )
            };
            menu.item(
                PopupMenuItem::new("Undo")
                    .action(Box::new(EditorUndo))
                    .disabled(!can_undo),
            )
            .item(
                PopupMenuItem::new("Redo")
                    .action(Box::new(EditorRedo))
                    .disabled(!can_redo),
            )
            .separator()
            .item(
                PopupMenuItem::new("Cut")
                    .action(Box::new(EditorCut))
                    .disabled(!has_selection),
            )
            .item(
                PopupMenuItem::new("Copy")
                    .action(Box::new(EditorCopy))
                    .disabled(!has_selection),
            )
            .item(
                PopupMenuItem::new("Paste")
                    .action(Box::new(EditorPaste))
                    .disabled(!has_clipboard_text),
            )
            .separator()
            .item(
                PopupMenuItem::new("Select All")
                    .action(Box::new(EditorSelectAll))
                    .disabled(!has_text),
            )
            .separator()
            .item(
                PopupMenuItem::new("Format Document")
                    .action(Box::new(crate::window::FormatDocument))
                    .disabled(!format_available),
            )
        });

    div()
        .size_full()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .px(px(18.))
        .pt(px(18.))
        .pb(px(12.))
        .bg(rgb(palette.canvas))
        .text_color(rgb(palette.text))
        .text_size(px(view_state.font_size))
        .child(editor_area)
        .children(
            window
                .find_widget
                .as_ref()
                .filter(|widget| widget.read(cx).is_visible())
                .cloned(),
        )
        .into_any_element()
}

fn render_preview_body(
    window: &mut DopadWindow,
    active_id: Option<u64>,
    font_size: f32,
    palette: &Palette,
    ui_window: &mut Window,
) -> gpui_kit::AnyElement {
    let preview_list = window.preview_list.clone();
    let revision = window
        .active_tab()
        .filter(|tab| Some(tab.id) == active_id)
        .and_then(|tab| tab.editor.as_ref())
        .map(dopad_editor::Editor::revision);
    let content = if let Some(content) = window
        .preview_cache
        .as_ref()
        .filter(|result| Some(result.document_id) == active_id && Some(result.revision) == revision)
        .map(|result| render_preview(&result.content, palette, ui_window, font_size, preview_list))
    {
        content
    } else if let Some(error) = window
        .preview_error
        .as_ref()
        .filter(|(id, error_revision, _)| {
            Some(*id) == active_id && Some(*error_revision) == revision
        })
        .map(|(_, _, error)| error.clone())
    {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(rgb(palette.canvas))
            .text_color(rgb(palette.text))
            .child("Preview could not be rendered")
            .child(div().text_sm().text_color(rgb(palette.muted)).child(error))
            .into_any_element()
    } else {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(palette.canvas))
            .text_color(rgb(palette.muted))
            .child("Preparing preview")
            .into_any_element()
    };
    div()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(palette.canvas))
        .child(div().flex_1().min_h_0().child(content))
        .into_any_element()
}
