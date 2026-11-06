use std::collections::HashMap;

use gpui_kit::component::IndexPath;
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use gpui_kit::{AppContext, Context, Entity, Subscription, Window};

use crate::{settings::AppSettings, window::shortcuts};

use super::{font_download, font_download::FontTarget, window::SettingsWindow};

pub(super) type SettingSelectState = SelectState<SearchableVec<String>>;

pub(super) struct Selectors {
    pub(super) editor_font: Entity<SettingSelectState>,
    pub(super) terminal_font: Entity<SettingSelectState>,
    pub(super) keyboard: HashMap<&'static str, Entity<SettingSelectState>>,
}

pub(super) fn initialize(
    settings: &AppSettings,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> (Selectors, Vec<Subscription>) {
    let installed_fonts = cx.text_system().all_font_names();
    let editor_fonts =
        font_download::font_options(installed_fonts.clone(), &settings.editor_font_family)
            .into_iter()
            .map(|(family, _)| family)
            .collect::<Vec<_>>();
    let terminal_fonts =
        font_download::font_options(installed_fonts, &settings.terminal_font_family)
            .into_iter()
            .map(|(family, _)| family)
            .collect::<Vec<_>>();

    let editor_font = new_select(editor_fonts, &settings.editor_font_family, window, cx);
    let terminal_font = new_select(terminal_fonts, &settings.terminal_font_family, window, cx);
    let mut subscriptions = Vec::with_capacity(shortcuts::specs().len() + 2);

    subscriptions.push(
        cx.subscribe_in(&editor_font, window, |this, _, event, _, cx| {
            if let SelectEvent::Confirm(Some(family)) = event {
                this.choose_font(family, FontTarget::Editor, cx);
            }
        }),
    );
    subscriptions.push(
        cx.subscribe_in(&terminal_font, window, |this, _, event, _, cx| {
            if let SelectEvent::Confirm(Some(family)) = event {
                this.choose_font(family, FontTarget::Terminal, cx);
            }
        }),
    );

    let mut keyboard = HashMap::with_capacity(shortcuts::specs().len());
    let choices = shortcuts::choice_keys()
        .iter()
        .map(|key| shortcuts::display_key(key))
        .collect::<Vec<_>>();

    for spec in shortcuts::specs() {
        let id = spec.id;
        let current = shortcuts::display_key(&shortcuts::effective_key(settings, id));
        let mut items = choices.clone();
        if !items.contains(&current) {
            items.push(current.clone());
        }
        let state = new_select(items, &current, window, cx);
        subscriptions.push(
            cx.subscribe_in(&state, window, move |this, _, event, _, cx| {
                let SelectEvent::Confirm(Some(label)) = event else {
                    return;
                };
                let Some(key) = shortcuts::key_from_display(label) else {
                    return;
                };
                if key == shortcuts::effective_default_key(id) {
                    this.draft.shortcut_overrides.remove(id);
                } else {
                    this.draft.shortcut_overrides.insert(id.to_owned(), key);
                }
                this.keyboard_notice = None;
                this.error = None;
                cx.notify();
            }),
        );
        keyboard.insert(id, state);
    }

    (
        Selectors {
            editor_font,
            terminal_font,
            keyboard,
        },
        subscriptions,
    )
}

fn new_select(
    items: Vec<String>,
    selected: &str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> Entity<SettingSelectState> {
    let selected_index = items
        .iter()
        .position(|item| item.eq_ignore_ascii_case(selected))
        .map(IndexPath::new);
    cx.new(|cx| {
        SelectState::new(SearchableVec::new(items), selected_index, window, cx).searchable(true)
    })
}
