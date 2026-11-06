use std::time::Instant;

use gpui_kit::{AppContext, Bounds, Entity, Pixels, Point, ScrollHandle, Window, point, px};

const EDGE_ZONE: f32 = 52.0;
const MIN_SPEED: f32 = 70.0;
const MAX_SPEED: f32 = 1_500.0;
const MIN_FRAME_SECONDS: f32 = 1.0 / 120.0;
const MAX_FRAME_SECONDS: f32 = 1.0 / 30.0;

#[derive(Default)]
pub(super) struct HoverScrollState {
    generation: u64,
    running: bool,
    last_frame: Option<Instant>,
}

impl HoverScrollState {
    pub(super) fn pointer_moved(
        &mut self,
        pointer: Point<Pixels>,
        scroll: &ScrollHandle,
    ) -> Option<u64> {
        if velocity_at(
            pointer,
            scroll.bounds(),
            scroll.offset().x,
            scroll.max_offset().x,
        )
        .is_none()
        {
            self.stop();
            return None;
        }

        if self.running {
            return None;
        }

        self.generation = self.generation.wrapping_add(1);
        self.running = true;
        self.last_frame = Some(Instant::now());
        Some(self.generation)
    }

    fn stop(&mut self) {
        self.last_frame = None;
        self.running = false;
        self.generation = self.generation.wrapping_add(1);
    }

    fn advance(
        &mut self,
        generation: u64,
        pointer: Point<Pixels>,
        scroll: &ScrollHandle,
    ) -> (bool, bool) {
        if !self.running || generation != self.generation {
            return (false, false);
        }

        let Some(velocity) = velocity_at(
            pointer,
            scroll.bounds(),
            scroll.offset().x,
            scroll.max_offset().x,
        ) else {
            self.stop();
            return (false, false);
        };

        let now = Instant::now();
        let seconds = self
            .last_frame
            .map(|last_frame| now.duration_since(last_frame).as_secs_f32())
            .unwrap_or(MIN_FRAME_SECONDS)
            .clamp(MIN_FRAME_SECONDS, MAX_FRAME_SECONDS);
        self.last_frame = Some(now);

        let offset = scroll.offset();
        let next_x = clamp_offset(offset.x + px(velocity * seconds), scroll.max_offset().x);
        if next_x == offset.x {
            self.stop();
            return (false, false);
        }

        scroll.set_offset(point(next_x, offset.y));
        (true, true)
    }
}

pub(super) fn cancel<C: AppContext>(state: &Entity<HoverScrollState>, cx: &mut C) {
    state.update(cx, |state, _| state.stop());
}

pub(super) fn schedule_frame<V: 'static>(
    window: &mut Window,
    state: Entity<HoverScrollState>,
    scroll: ScrollHandle,
    view: Entity<V>,
    generation: u64,
) {
    window.on_next_frame(move |window, cx| {
        let pointer = window.mouse_position();
        let (continue_scrolling, moved) =
            state.update(cx, |state, _| state.advance(generation, pointer, &scroll));
        if moved {
            cx.notify(view.entity_id());
        }
        if continue_scrolling {
            schedule_frame(window, state, scroll, view, generation);
        }
    });
}

fn velocity_at(
    pointer: Point<Pixels>,
    bounds: Bounds<Pixels>,
    offset_x: Pixels,
    max_offset_x: Pixels,
) -> Option<f32> {
    let pointer_x = f32::from(pointer.x);
    let pointer_y = f32::from(pointer.y);
    if pointer_y < f32::from(bounds.top()) || pointer_y > f32::from(bounds.bottom()) {
        return None;
    }
    let left_distance = pointer_x - f32::from(bounds.left());
    let right_distance = f32::from(bounds.right()) - pointer_x;
    if left_distance < 0.0 || right_distance < 0.0 {
        return None;
    }
    let current_offset = f32::from(offset_x);
    let min_offset = -f32::from(max_offset_x);
    let can_scroll_left = current_offset < 0.0;
    let can_scroll_right = current_offset > min_offset;
    let left_edge = left_distance <= EDGE_ZONE && can_scroll_left;
    let right_edge = right_distance <= EDGE_ZONE && can_scroll_right;

    let (distance, direction) = match (left_edge, right_edge) {
        (true, true) if left_distance < right_distance => (left_distance, 1.0),
        (true, true) => (right_distance, -1.0),
        (true, false) => (left_distance, 1.0),
        (false, true) => (right_distance, -1.0),
        (false, false) => return None,
    };

    let proximity = ((EDGE_ZONE - distance) / EDGE_ZONE).clamp(0.0, 1.0);
    Some(direction * (MIN_SPEED + (MAX_SPEED - MIN_SPEED) * proximity * proximity))
}

fn clamp_offset(offset: Pixels, max_offset: Pixels) -> Pixels {
    offset.min(px(0.)).max(-max_offset)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, Pixels, point, px, size};

    use super::{clamp_offset, velocity_at};

    fn bounds() -> Bounds<Pixels> {
        Bounds {
            origin: point(px(100.), px(0.)),
            size: size(px(400.), px(44.)),
        }
    }

    #[test]
    fn edge_velocity_scales_and_uses_the_expected_direction() {
        let bounds = bounds();
        let near_left = velocity_at(point(px(101.), px(22.)), bounds, px(-200.), px(2_000.))
            .expect("left edge scrolls toward the beginning");
        let near_right = velocity_at(point(px(499.), px(22.)), bounds, px(-200.), px(2_000.))
            .expect("right edge scrolls toward later tabs");
        let outer_left = velocity_at(point(px(149.), px(22.)), bounds, px(-200.), px(2_000.))
            .expect("edge zone includes its inner boundary");

        assert!(near_left > 0.0);
        assert!(near_right < 0.0);
        assert!(near_left.abs() > outer_left.abs());
        assert_eq!(
            velocity_at(point(px(300.), px(22.)), bounds, px(-200.), px(2_000.)),
            None
        );
        assert_eq!(
            velocity_at(point(px(101.), px(60.)), bounds, px(-200.), px(2_000.)),
            None,
            "leaving the tab viewport vertically stops scrolling"
        );
    }

    #[test]
    fn edge_velocity_stops_at_both_scroll_limits() {
        let bounds = bounds();
        assert_eq!(
            velocity_at(point(px(101.), px(22.)), bounds, px(0.), px(2_000.)),
            None
        );
        assert_eq!(
            velocity_at(point(px(499.), px(22.)), bounds, px(-2_000.), px(2_000.)),
            None
        );
    }

    #[test]
    fn offset_is_clamped_to_the_scrollable_range() {
        assert_eq!(clamp_offset(px(12.), px(800.)), px(0.));
        assert_eq!(clamp_offset(px(-900.), px(800.)), px(-800.));
        assert_eq!(clamp_offset(px(-300.), px(800.)), px(-300.));
    }
}
