use gpui_kit::Anchor;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::{Context, IntoElement};

use crate::settings::AppSettings;

use super::shortcuts::{display_key, effective_key};
use super::{
    CloseDocument, DopadWindow, DuplicateLine, Find, GoToLine, MoveLineDown, MoveLineUp,
    NewDocument, OpenDocument, OpenFolder, Replace, ResetZoom, SaveAsDocument, SaveDocument,
    ToggleComment, TogglePreview, ZoomIn, ZoomOut, preview,
};

pub(super) fn render(
    window: &DopadWindow,
    _cx: &mut Context<DopadWindow>,
) -> impl IntoElement + use<> {
    let settings = window.settings.current();
    let preview_active = window.active_tab().is_some_and(|tab| tab.preview_mode);
    let preview_available = window
        .active_tab()
        .is_some_and(preview::supports_tab_preview);

    Button::new("application-menu")
        .icon(IconName::ChevronDown)
        .compact()
        .ghost()
        .tooltip("Application menu")
        .accessibility_label("Open application menu")
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
            let help_settings = settings.clone();
            menu.submenu("File", window, cx, |menu, _, _| {
                menu.item(PopupMenuItem::new("New").action(Box::new(NewDocument)))
                    .item(PopupMenuItem::new("Open…").action(Box::new(OpenDocument)))
                    .item(PopupMenuItem::new("Open Folder…").action(Box::new(OpenFolder)))
                    .separator()
                    .item(PopupMenuItem::new("Save").action(Box::new(SaveDocument)))
                    .item(PopupMenuItem::new("Save As…").action(Box::new(SaveAsDocument)))
                    .separator()
                    .item(PopupMenuItem::new("Close Tab").action(Box::new(CloseDocument)))
            })
            .submenu("Edit", window, cx, |menu, _, _| {
                menu.item(PopupMenuItem::new("Find").action(Box::new(Find)))
                    .item(PopupMenuItem::new("Replace").action(Box::new(Replace)))
                    .item(PopupMenuItem::new("Go to Line or Column…").action(Box::new(GoToLine)))
                    .separator()
                    .item(PopupMenuItem::new("Toggle Comment").action(Box::new(ToggleComment)))
                    .item(PopupMenuItem::new("Duplicate Line").action(Box::new(DuplicateLine)))
                    .item(PopupMenuItem::new("Move Line Up").action(Box::new(MoveLineUp)))
                    .item(PopupMenuItem::new("Move Line Down").action(Box::new(MoveLineDown)))
            })
            .submenu("View", window, cx, move |menu, _, _| {
                let menu = if preview_available {
                    menu.item(
                        PopupMenuItem::new("Preview")
                            .checked(preview_active)
                            .action(Box::new(TogglePreview)),
                    )
                    .separator()
                } else {
                    menu
                };
                menu.item(PopupMenuItem::new("Zoom In").action(Box::new(ZoomIn)))
                    .item(PopupMenuItem::new("Zoom Out").action(Box::new(ZoomOut)))
                    .item(PopupMenuItem::new("Reset Zoom").action(Box::new(ResetZoom)))
            })
            .item(PopupMenuItem::new("Settings").action(Box::new(super::OpenSettings)))
            .separator()
            .submenu("Help", window, cx, move |menu, _, _| {
                menu.label("Keyboard shortcuts")
                    .separator()
                    .item(shortcut_item(
                        &help_settings,
                        "new_document",
                        "New document",
                    ))
                    .item(shortcut_item(
                        &help_settings,
                        "open_document",
                        "Open document",
                    ))
                    .item(shortcut_item(&help_settings, "save", "Save"))
                    .item(shortcut_item(&help_settings, "save_as", "Save As"))
                    .item(shortcut_item(&help_settings, "close_tab", "Close tab"))
                    .separator()
                    .item(shortcut_item(&help_settings, "find", "Find"))
                    .item(shortcut_item(&help_settings, "replace", "Replace"))
                    .item(shortcut_item(&help_settings, "toggle_wrap", "Word Wrap"))
                    .item(shortcut_item(&help_settings, "zoom_in", "Zoom In"))
                    .item(shortcut_item(&help_settings, "zoom_out", "Zoom Out"))
                    .item(shortcut_item(&help_settings, "reset_zoom", "Reset Zoom"))
                    .item(shortcut_item(&help_settings, "preview", "Preview"))
                    .separator()
                    .item(shortcut_item(
                        &help_settings,
                        "go_to_line",
                        "Go to Line or Column",
                    ))
                    .item(shortcut_item(
                        &help_settings,
                        "toggle_comment",
                        "Toggle Comment",
                    ))
                    .item(shortcut_item(
                        &help_settings,
                        "duplicate_line",
                        "Duplicate Line",
                    ))
                    .item(shortcut_item(
                        &help_settings,
                        "move_line_up",
                        "Move Line Up",
                    ))
                    .item(shortcut_item(
                        &help_settings,
                        "move_line_down",
                        "Move Line Down",
                    ))
                    .item(shortcut_item(
                        &help_settings,
                        "format_document",
                        "Format Document",
                    ))
                    .scrollable(true)
            })
        })
}

fn shortcut_item(settings: &AppSettings, id: &str, label: &str) -> PopupMenuItem {
    PopupMenuItem::label(format!(
        "{label} ({})",
        display_key(&effective_key(settings, id))
    ))
}
