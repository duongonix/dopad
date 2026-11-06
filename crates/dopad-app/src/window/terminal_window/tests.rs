use super::{scaled_cell_width, terminal_line_height};

#[test]
fn terminal_cell_width_scales_measured_monospace_advance() {
    assert_eq!(scaled_cell_width(8.0, 100), 8.0);
    assert_eq!(scaled_cell_width(8.0, 125), 10.0);
    assert_eq!(scaled_cell_width(0.0, 100), 1.0);
}

#[test]
fn terminal_line_height_tracks_font_size_and_configured_ratio() {
    assert!((terminal_line_height(13, 140) - 18.2).abs() < f32::EPSILON * 32.0);
    assert_eq!(terminal_line_height(16, 150), 24.0);
    assert_eq!(terminal_line_height(16, 100), 16.0);
}
