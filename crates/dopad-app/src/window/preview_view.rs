use std::sync::Arc;

use dopad_preview::{
    InlineSpan, MarkdownBlock, MarkdownDocument, PreviewContent, PreviewWarning, SvgDocument,
};
use gpui_kit::component::{link::Link, scroll::ScrollableElement};
use gpui_kit::{
    AnyElement, ElementId, FontWeight, Image, ImageFormat, IntoElement, ListState, ObjectFit,
    ParentElement, Styled, StyledImage, Window, div, img, list, px, relative, rgb,
};

use crate::theme::Palette;

#[cfg(test)]
use gpui_kit::{InteractiveElement, TestSupportExt};

pub(super) fn render_preview(
    content: &PreviewContent,
    palette: &Palette,
    window: &Window,
    font_size: f32,
    list_state: ListState,
) -> AnyElement {
    match content {
        PreviewContent::Markdown(document) => {
            render_markdown(document, *palette, font_size, list_state).into_any_element()
        }
        PreviewContent::Svg(document) => {
            render_svg(document, palette, window, font_size).into_any_element()
        }
    }
}

fn render_markdown(
    document: &Arc<MarkdownDocument>,
    palette: Palette,
    font_size: f32,
    list_state: ListState,
) -> impl gpui_kit::IntoElement {
    let item_count = document
        .warnings
        .len()
        .saturating_add(document.blocks.len());
    let current_count = list_state.item_count();
    if current_count != item_count {
        list_state.splice(0..current_count, item_count);
    }
    if document.blocks.is_empty() && document.warnings.is_empty() {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(palette.canvas))
            .text_color(rgb(palette.muted))
            .child("This Markdown file is empty.");
    }
    let warning_count = document.warnings.len();
    let document = Arc::clone(document);
    let scrollbar_state = list_state.clone();
    let blocks = list(list_state, move |index, _, _| {
        if let Some(warning) = document.warnings.get(index) {
            return div()
                .w_full()
                .pb(px(16.))
                .text_sm()
                .text_color(rgb(palette.muted))
                .child(warning_text(*warning))
                .into_any_element();
        }
        let block_index = index.saturating_sub(warning_count);
        document.blocks.get(block_index).map_or_else(
            || div().into_any_element(),
            |block| {
                let row = div().w_full().pb(px(16.)).child(render_block(
                    block,
                    &palette,
                    font_size,
                    block_index,
                ));
                #[cfg(test)]
                let row = row
                    .id(format!("dopad-preview-block-{block_index}"))
                    .debug_selector(move || format!("dopad-preview-block-{block_index}"))
                    .test_support();
                row.into_any_element()
            },
        )
    });
    div()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(palette.canvas))
        .text_color(rgb(palette.text))
        .px_6()
        .py_4()
        .child(blocks.flex_1().min_h_0().w_full())
        .vertical_scrollbar(&scrollbar_state)
}

fn render_block(
    block: &MarkdownBlock,
    palette: &Palette,
    font_size: f32,
    block_index: usize,
) -> gpui_kit::Div {
    match block {
        MarkdownBlock::Heading { level, spans } => {
            let mut element = div().w_full().flex().flex_wrap().items_center();
            element = match level {
                1 => element
                    .text_size(px(font_size * 2.0))
                    .line_height(relative(1.2))
                    .font_weight(FontWeight::SEMIBOLD),
                2 => element
                    .text_size(px(font_size * 1.5))
                    .line_height(relative(1.25))
                    .font_weight(FontWeight::SEMIBOLD),
                3 => element
                    .text_size(px(font_size * 1.2))
                    .font_weight(FontWeight::SEMIBOLD),
                _ => element
                    .text_size(px(font_size * 1.05))
                    .font_weight(FontWeight::SEMIBOLD),
            };
            element.text_color(rgb(palette.text)).children(render_spans(
                spans,
                palette,
                block_index,
                0,
            ))
        }
        MarkdownBlock::Paragraph { spans } => div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .text_size(px(font_size))
            .line_height(relative(1.5))
            .children(render_spans(spans, palette, block_index, 0)),
        MarkdownBlock::Code { language, text } => {
            let language_label = language
                .as_deref()
                .filter(|language| !language.is_empty())
                .map(|language| {
                    div()
                        .text_size(px(font_size * 0.8))
                        .text_color(rgb(palette.muted))
                        .child(language.to_owned())
                });
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_sm()
                .bg(rgb(palette.chrome))
                .px_4()
                .py_3()
                .children(language_label)
                .child(div().text_size(px(font_size * 0.9)).child(text.clone()))
        }
        MarkdownBlock::Quote { spans } => div()
            .w_full()
            .flex()
            .gap_3()
            .border_l_2()
            .border_color(rgb(palette.border))
            .pl_3()
            .text_size(px(font_size))
            .text_color(rgb(palette.muted))
            .children(render_spans(spans, palette, block_index, 0)),
        MarkdownBlock::ListItem {
            depth,
            ordered,
            number,
            checked,
            spans,
        } => {
            let marker = if *ordered {
                div()
                    .w(px(24.))
                    .text_color(rgb(palette.muted))
                    .child(number.unwrap_or(1).to_string())
            } else if checked.is_some() {
                let mut box_mark = div()
                    .size(px(12.))
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(palette.muted));
                if checked == &Some(true) {
                    box_mark = box_mark.bg(rgb(palette.accent));
                }
                div().w(px(24.)).child(box_mark)
            } else {
                div()
                    .w(px(24.))
                    .flex()
                    .items_center()
                    .child(div().size(px(5.)).rounded_full().bg(rgb(palette.accent)))
            };
            div()
                .w_full()
                .flex()
                .gap_2()
                .text_size(px(font_size))
                .ml(px((*depth as f32 * 16.0).min(256.)))
                .child(marker)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .text_size(px(font_size))
                        .children(render_spans(spans, palette, block_index, 0)),
                )
        }
        MarkdownBlock::TableRow { header, cells } => {
            div()
                .w_full()
                .flex()
                .gap_4()
                .children(cells.iter().enumerate().map(|(cell_index, cell)| {
                    let mut element = div()
                        .flex_1()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .children(render_spans(cell, palette, block_index, cell_index));
                    if *header {
                        element = element.font_weight(FontWeight::SEMIBOLD);
                    }
                    element
                }))
        }
        MarkdownBlock::Rule => div().w_full().h(px(1.)).bg(rgb(palette.border)),
    }
}

fn render_spans<'a>(
    spans: &'a [InlineSpan],
    palette: &Palette,
    block_index: usize,
    sub_index: usize,
) -> impl Iterator<Item = AnyElement> + 'a {
    let accent = palette.accent;
    let code_background = palette.selection;
    let code_color = palette.accent;
    spans.iter().enumerate().map(move |(span_index, span)| {
        let mut element = div().child(span.text.clone());
        if span.style.strong {
            element = element.font_weight(FontWeight::SEMIBOLD);
        }
        if span.style.emphasis {
            element = element.italic();
        }
        if span.style.code {
            element = element
                .rounded_sm()
                .bg(rgb(code_background))
                .px_1()
                .text_size(px(13.))
                .text_color(rgb(code_color));
        }
        if span.style.strikethrough {
            element = element.line_through();
        }
        if let Some(target) = span.link.as_ref().filter(|target| {
            target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with("mailto:")
        }) {
            return Link::new(ElementId::Name(
                format!("preview-link-{block_index}-{sub_index}-{span_index}").into(),
            ))
            .href(target.clone())
            .text_color(rgb(accent))
            .child(element)
            .into_any_element();
        }
        element.into_any_element()
    })
}

fn render_svg(
    document: &SvgDocument,
    palette: &Palette,
    window: &Window,
    font_size: f32,
) -> impl gpui_kit::IntoElement {
    let bounds = window.bounds().size;
    let max_width = (bounds.width.as_f32() - 80.0).max(1.0);
    let max_height = (bounds.height.as_f32() - 150.0).max(1.0);
    let fit = (max_width / document.width as f32)
        .min(max_height / document.height as f32)
        .min(1.0)
        * (font_size / 14.0);
    let image = Image::from_bytes(ImageFormat::Png, document.png.clone());
    div()
        .size_full()
        .overflow_scrollbar()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(palette.canvas))
        .child(
            img(Arc::new(image))
                .size(px(document.width as f32 * fit))
                .h(px(document.height as f32 * fit))
                .object_fit(ObjectFit::Contain),
        )
}

fn warning_text(warning: PreviewWarning) -> &'static str {
    match warning {
        PreviewWarning::RawHtmlOmitted => "Raw HTML is omitted from the native preview.",
        PreviewWarning::ImagesOmitted => "Markdown images are not loaded in preview.",
        PreviewWarning::UnsafeLinkOmitted => "Unsafe link destinations were removed.",
        PreviewWarning::UnsupportedMarkdownOmitted => "Some Markdown syntax is not rendered.",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dopad_preview::InlineStyle;
    use gpui::{Context, ListAlignment, Render, TestAppContext, Window, px, size};
    use gpui_kit::{IntoElement, ListState};

    use super::{InlineSpan, MarkdownBlock, MarkdownDocument, render_markdown};

    struct MarkdownPreviewHarness {
        document: Arc<MarkdownDocument>,
        list_state: ListState,
    }

    impl Render for MarkdownPreviewHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            render_markdown(
                &self.document,
                *crate::theme::palette(false),
                14.,
                self.list_state.clone(),
            )
        }
    }

    #[gpui::test]
    fn markdown_blocks_remain_visible_after_virtual_list_is_populated(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let document = Arc::new(MarkdownDocument {
            blocks: vec![MarkdownBlock::Paragraph {
                spans: vec![InlineSpan {
                    text: "Preview content must be visible".into(),
                    style: InlineStyle::default(),
                    link: None,
                }],
            }],
            warnings: Vec::new(),
        });
        let list_state = ListState::new(0, ListAlignment::Top, px(240.));
        let (view, test_window) = cx.add_window_view(move |_, _| MarkdownPreviewHarness {
            document,
            list_state: list_state.clone(),
        });
        test_window.simulate_resize(size(px(640.), px(420.)));
        view.update(test_window, |_, cx| cx.notify());
        test_window.run_until_parked();
        for _ in 0..3 {
            test_window.update(|window, cx| {
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx);
            });
        }

        let bounds = test_window
            .debug_bounds("dopad-preview-block-0")
            .expect("the Markdown block should be laid out in the preview");
        assert!(bounds.size.height > px(0.));
    }
}
