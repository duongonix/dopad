use crate::theme::Palette;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TabIndicator {
    Empty,
    Dirty,
    Close,
}

pub(super) fn tab_indicator(active: bool, dirty: bool, hovered: bool) -> TabIndicator {
    if hovered || (active && !dirty) {
        TabIndicator::Close
    } else if dirty {
        TabIndicator::Dirty
    } else {
        TabIndicator::Empty
    }
}

pub(super) fn tab_padding(compact: bool) -> (f32, f32) {
    if compact { (9., 6.) } else { (15., 12.) }
}

pub(super) fn tab_hover_background(active: bool, palette: &Palette) -> Option<u32> {
    (!active).then_some(palette.tab_hover)
}
