#![forbid(unsafe_code)]

//! Bounded native preview models. No UI, file, or network dependencies.

use std::sync::Arc;

mod markdown;
mod svg;

use dopad_core::Revision;

pub use markdown::{InlineSpan, InlineStyle, MarkdownBlock, MarkdownDocument, PreviewWarning};
pub use svg::{SvgDocument, SvgPolicy};

#[derive(Clone, Debug)]
pub enum PreviewContent {
    Markdown(Arc<MarkdownDocument>),
    Svg(SvgDocument),
}

#[derive(Clone, Debug)]
pub struct PreviewResult {
    pub document_id: u64,
    pub revision: Revision,
    pub content: PreviewContent,
}

#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error("preview was cancelled")]
    Cancelled,
    #[error("document exceeds the preview input limit")]
    InputTooLarge,
    #[error("Markdown parse failed: {0}")]
    MarkdownParse(String),
    #[error("preview exceeds a structural resource limit")]
    ResourceLimit,
    #[error("SVG contains a blocked active or external resource")]
    UnsafeSvg,
    #[error("SVG is malformed: {0}")]
    SvgParse(String),
    #[error("SVG dimensions are empty, invalid, or exceed the raster budget")]
    InvalidSvgSize,
    #[error("SVG rasterization failed: {0}")]
    SvgRender(String),
}

pub fn render_markdown(
    document_id: u64,
    revision: Revision,
    text: &str,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<PreviewResult, PreviewError> {
    let content = markdown::parse_markdown(text, cancelled)?;
    Ok(PreviewResult {
        document_id,
        revision,
        content: PreviewContent::Markdown(Arc::new(content)),
    })
}

pub fn render_svg(
    document_id: u64,
    revision: Revision,
    text: &str,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<PreviewResult, PreviewError> {
    let content = svg::render_svg(text, &SvgPolicy::default(), cancelled)?;
    Ok(PreviewResult {
        document_id,
        revision,
        content: PreviewContent::Svg(content),
    })
}
