use std::collections::VecDeque;

use gpui_kit::{Context, Window};

use super::{DopadWindow, dialogs};

impl DopadWindow {
    pub(super) fn request_close_tab(
        &mut self,
        document_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == document_id) else {
            return;
        };
        if !tab.is_dirty() || !self.settings.current().confirm_before_close {
            self.remove_tab(document_id);
            cx.notify();
            return;
        }
        dialogs::confirm_close_tab(window, cx, document_id, tab.title.clone(), false);
    }

    pub(super) fn close_tabs_from_context(
        &mut self,
        document_ids: Vec<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if document_ids.is_empty() {
            return;
        }
        self.pending_close_tabs = Some(VecDeque::from(document_ids));
        self.close_batch_resume = false;
        self.continue_close_batch(window, cx);
    }

    pub(super) fn continue_close_batch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        loop {
            let Some(document_id) = self
                .pending_close_tabs
                .as_ref()
                .and_then(|tabs| tabs.front().copied())
            else {
                self.cancel_close_batch();
                cx.notify();
                return;
            };

            let Some(index) = self.tabs.iter().position(|tab| tab.id == document_id) else {
                if let Some(tabs) = self.pending_close_tabs.as_mut() {
                    tabs.pop_front();
                }
                continue;
            };

            let tab = &self.tabs[index];
            if tab.is_dirty() && self.settings.current().confirm_before_close {
                dialogs::confirm_close_tab(window, cx, document_id, tab.title.clone(), true);
                return;
            }

            if let Some(tabs) = self.pending_close_tabs.as_mut() {
                tabs.pop_front();
            }
            self.remove_tab(document_id);
        }
    }

    pub(super) fn cancel_close_batch(&mut self) {
        self.pending_close_tabs = None;
        self.close_batch_resume = false;
    }
}
