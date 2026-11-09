use gpui_kit::{
    Context, IntoElement, ParentElement, Pixels, Point, Render, Styled, Window, div, px, rgb,
};

#[derive(Clone)]
pub(super) struct DraggedTab {
    pub(super) document_id: u64,
    title: String,
    canvas: u32,
    text: u32,
    border: u32,
}

impl DraggedTab {
    pub(super) fn new(
        document_id: u64,
        title: String,
        canvas: u32,
        text: u32,
        border: u32,
    ) -> Self {
        Self {
            document_id,
            title,
            canvas,
            text,
            border,
        }
    }

    pub(super) fn preview(&self, position: Point<Pixels>) -> TabDragPreview {
        TabDragPreview {
            position,
            title: self.title.clone(),
            canvas: self.canvas,
            text: self.text,
            border: self.border,
        }
    }
}

pub(super) struct TabDragPreview {
    position: Point<Pixels>,
    title: String,
    canvas: u32,
    text: u32,
    border: u32,
}

impl Render for TabDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let width = px(150.);
        let height = px(28.);
        div()
            .pl(self.position.x - px(75.))
            .pt(self.position.y - px(14.))
            .child(
                div()
                    .w(width)
                    .h(height)
                    .px_3()
                    .flex()
                    .items_center()
                    .overflow_x_hidden()
                    .border_1()
                    .border_color(rgb(self.border))
                    .bg(rgb(self.canvas))
                    .text_color(rgb(self.text))
                    .text_sm()
                    .text_ellipsis()
                    .child(self.title.clone()),
            )
    }
}

pub(super) fn reorder_tabs<T>(
    tabs: &mut Vec<T>,
    active: &mut usize,
    dragged_id: u64,
    target_id: u64,
    id: impl Fn(&T) -> u64,
) -> bool {
    let Some(source_index) = tabs.iter().position(|tab| id(tab) == dragged_id) else {
        return false;
    };
    let Some(target_index) = tabs.iter().position(|tab| id(tab) == target_id) else {
        return false;
    };
    if source_index == target_index {
        return false;
    }

    let active_id = tabs.get(*active).map(&id);
    let item = tabs.remove(source_index);
    let target_after_removal = tabs
        .iter()
        .position(|tab| id(tab) == target_id)
        .unwrap_or(tabs.len());
    let insertion_index = if source_index < target_index {
        target_after_removal + 1
    } else {
        target_after_removal
    };
    tabs.insert(insertion_index, item);

    if let Some(active_id) = active_id
        && let Some(active_index) = tabs.iter().position(|tab| id(tab) == active_id)
    {
        *active = active_index;
    } else {
        *active = (*active).min(tabs.len().saturating_sub(1));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::reorder_tabs;

    #[test]
    fn dropping_on_a_later_tab_moves_after_it_and_preserves_active_document() {
        let mut tabs = vec![10, 20, 30, 40];
        let mut active = 0;

        assert!(reorder_tabs(&mut tabs, &mut active, 10, 30, |tab| *tab));

        assert_eq!(tabs, vec![20, 30, 10, 40]);
        assert_eq!(tabs[active], 10);
    }

    #[test]
    fn dropping_on_an_earlier_tab_moves_before_it_and_preserves_active_document() {
        let mut tabs = vec![10, 20, 30, 40];
        let mut active = 1;

        assert!(reorder_tabs(&mut tabs, &mut active, 40, 20, |tab| *tab));

        assert_eq!(tabs, vec![10, 40, 20, 30]);
        assert_eq!(tabs[active], 20);
    }

    #[test]
    fn dropping_on_self_or_unknown_tabs_does_not_mutate_order() {
        let mut tabs = vec![10, 20, 30];
        let mut active = 1;

        assert!(!reorder_tabs(&mut tabs, &mut active, 20, 20, |tab| *tab));
        assert!(!reorder_tabs(&mut tabs, &mut active, 99, 10, |tab| *tab));
        assert!(!reorder_tabs(&mut tabs, &mut active, 20, 99, |tab| *tab));
        assert_eq!(tabs, vec![10, 20, 30]);
        assert_eq!(active, 1);
    }
}
