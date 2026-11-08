use std::sync::Arc;

use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::{Entity, IntoElement};

use super::super::DopadWindow;

#[derive(Clone)]
enum CloseAction {
    One(u64),
    Many(Vec<u64>),
}

pub(super) fn attach<E>(
    tab: E,
    view: Entity<DopadWindow>,
    target_id: u64,
    target_index: usize,
    tab_ids: Arc<Vec<u64>>,
) -> impl IntoElement
where
    E: ContextMenuExt + IntoElement + 'static,
{
    tab.context_menu(move |mut menu, _, _| {
        let others = tab_ids
            .iter()
            .copied()
            .filter(|id| *id != target_id)
            .collect::<Vec<_>>();
        let left = tab_ids
            .iter()
            .take(target_index)
            .copied()
            .collect::<Vec<_>>();
        let right = tab_ids
            .iter()
            .skip(target_index.saturating_add(1))
            .copied()
            .collect::<Vec<_>>();

        menu = menu
            .item(close_item(
                "Close Tab",
                view.clone(),
                CloseAction::One(target_id),
                false,
            ))
            .item(close_item(
                "Close Others",
                view.clone(),
                CloseAction::Many(others.clone()),
                others.is_empty(),
            ))
            .item(close_item(
                "Close All",
                view.clone(),
                CloseAction::Many(tab_ids.as_ref().clone()),
                false,
            ))
            .separator()
            .item(close_item(
                "Close Tabs to the Right",
                view.clone(),
                CloseAction::Many(right.clone()),
                right.is_empty(),
            ))
            .item(close_item(
                "Close Tabs to the Left",
                view.clone(),
                CloseAction::Many(left.clone()),
                left.is_empty(),
            ));
        menu
    })
}

fn close_item(
    label: &'static str,
    view: Entity<DopadWindow>,
    action: CloseAction,
    disabled: bool,
) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .disabled(disabled)
        .on_click(move |_, window, app| {
            view.update(app, |state, cx| match action.clone() {
                CloseAction::One(document_id) => {
                    state.request_close_tab(document_id, window, cx);
                }
                CloseAction::Many(document_ids) => {
                    state.close_tabs_from_context(document_ids, window, cx);
                }
            });
        })
}
