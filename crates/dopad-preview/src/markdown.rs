use std::sync::atomic::{AtomicBool, Ordering};

use markdown::mdast::Node;

use crate::PreviewError;

const MAX_MARKDOWN_BYTES: usize = 2 * 1024 * 1024;
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 128;
const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InlineStyle {
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
    pub strikethrough: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineSpan {
    pub text: String,
    pub style: InlineStyle,
    /// Safe destination metadata. Consumers must only open it after an explicit click.
    pub link: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkdownBlock {
    Heading {
        level: u8,
        spans: Vec<InlineSpan>,
    },
    Paragraph {
        spans: Vec<InlineSpan>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Quote {
        spans: Vec<InlineSpan>,
    },
    ListItem {
        depth: usize,
        ordered: bool,
        number: Option<u32>,
        checked: Option<bool>,
        spans: Vec<InlineSpan>,
    },
    TableRow {
        header: bool,
        cells: Vec<Vec<InlineSpan>>,
    },
    Rule,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewWarning {
    RawHtmlOmitted,
    ImagesOmitted,
    UnsafeLinkOmitted,
    UnsupportedMarkdownOmitted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkdownDocument {
    pub blocks: Vec<MarkdownBlock>,
    pub warnings: Vec<PreviewWarning>,
}

struct Builder {
    document: MarkdownDocument,
    nodes: usize,
    output_bytes: usize,
}

impl Builder {
    fn visit(&mut self, node: &Node, depth: usize) -> Result<(), PreviewError> {
        self.bump(depth)?;
        match node {
            Node::Root(root) => {
                for child in &root.children {
                    self.visit(child, depth + 1)?;
                }
            }
            Node::Heading(heading) => {
                let spans =
                    self.inline(&heading.children, depth + 1, InlineStyle::default(), None)?;
                self.document.blocks.push(MarkdownBlock::Heading {
                    level: heading.depth,
                    spans,
                });
            }
            Node::Paragraph(paragraph) => {
                let spans =
                    self.inline(&paragraph.children, depth + 1, InlineStyle::default(), None)?;
                self.document
                    .blocks
                    .push(MarkdownBlock::Paragraph { spans });
            }
            Node::Code(code) => {
                self.add_output(code.value.len())?;
                self.document.blocks.push(MarkdownBlock::Code {
                    language: code.lang.clone(),
                    text: code.value.clone(),
                });
            }
            Node::Blockquote(quote) => {
                let mut spans = Vec::new();
                for child in &quote.children {
                    self.append_block_inline(child, depth + 1, &mut spans)?;
                }
                self.document.blocks.push(MarkdownBlock::Quote { spans });
            }
            Node::List(list) => {
                for (index, item) in list.children.iter().enumerate() {
                    self.bump(depth + 1)?;
                    if let Node::ListItem(item) = item {
                        let mut spans = Vec::new();
                        for child in &item.children {
                            self.append_block_inline(child, depth + 2, &mut spans)?;
                        }
                        self.document.blocks.push(MarkdownBlock::ListItem {
                            depth,
                            ordered: list.ordered,
                            number: list
                                .ordered
                                .then(|| list.start.unwrap_or(1).saturating_add(index as u32)),
                            checked: item.checked,
                            spans,
                        });
                    }
                }
            }
            Node::Table(table) => {
                for (row_index, row) in table.children.iter().enumerate() {
                    self.bump(depth + 1)?;
                    if let Node::TableRow(row) = row {
                        let mut cells = Vec::with_capacity(row.children.len());
                        for cell in &row.children {
                            self.bump(depth + 2)?;
                            let spans = match cell {
                                Node::TableCell(cell) => self.inline(
                                    &cell.children,
                                    depth + 3,
                                    InlineStyle::default(),
                                    None,
                                )?,
                                _ => Vec::new(),
                            };
                            cells.push(spans);
                        }
                        self.document.blocks.push(MarkdownBlock::TableRow {
                            header: row_index == 0,
                            cells,
                        });
                    }
                }
            }
            Node::ThematicBreak(_) => self.document.blocks.push(MarkdownBlock::Rule),
            Node::Html(_) => self.warn(PreviewWarning::RawHtmlOmitted),
            Node::Definition(_) | Node::LinkReference(_) | Node::ImageReference(_) => {}
            _ => self.warn(PreviewWarning::UnsupportedMarkdownOmitted),
        }
        Ok(())
    }

    fn append_block_inline(
        &mut self,
        node: &Node,
        depth: usize,
        spans: &mut Vec<InlineSpan>,
    ) -> Result<(), PreviewError> {
        self.bump(depth)?;
        match node {
            Node::Paragraph(paragraph) => spans.extend(self.inline(
                &paragraph.children,
                depth + 1,
                InlineStyle::default(),
                None,
            )?),
            Node::List(list) => {
                for item in &list.children {
                    self.append_block_inline(item, depth + 1, spans)?;
                }
            }
            Node::ListItem(item) => {
                for child in &item.children {
                    self.append_block_inline(child, depth + 1, spans)?;
                }
            }
            Node::Blockquote(quote) => {
                for child in &quote.children {
                    self.append_block_inline(child, depth + 1, spans)?;
                }
            }
            _ => spans.extend(self.inline(
                std::slice::from_ref(node),
                depth + 1,
                InlineStyle::default(),
                None,
            )?),
        }
        Ok(())
    }

    fn inline(
        &mut self,
        nodes: &[Node],
        depth: usize,
        style: InlineStyle,
        link: Option<String>,
    ) -> Result<Vec<InlineSpan>, PreviewError> {
        let mut spans = Vec::new();
        for node in nodes {
            self.bump(depth)?;
            match node {
                Node::Text(text) => {
                    self.push_span(&mut spans, &text.value, &style, link.clone())?
                }
                Node::InlineCode(code) => {
                    let mut code_style = style.clone();
                    code_style.code = true;
                    self.push_span(&mut spans, &code.value, &code_style, link.clone())?;
                }
                Node::Emphasis(value) => {
                    let mut nested = style.clone();
                    nested.emphasis = true;
                    spans.extend(self.inline(&value.children, depth + 1, nested, link.clone())?);
                }
                Node::Strong(value) => {
                    let mut nested = style.clone();
                    nested.strong = true;
                    spans.extend(self.inline(&value.children, depth + 1, nested, link.clone())?);
                }
                Node::Delete(value) => {
                    let mut nested = style.clone();
                    nested.strikethrough = true;
                    spans.extend(self.inline(&value.children, depth + 1, nested, link.clone())?);
                }
                Node::Link(value) => {
                    if is_safe_link(&value.url) {
                        spans.extend(self.inline(
                            &value.children,
                            depth + 1,
                            style.clone(),
                            Some(value.url.clone()),
                        )?);
                    } else {
                        self.warn(PreviewWarning::UnsafeLinkOmitted);
                        spans.extend(self.inline(
                            &value.children,
                            depth + 1,
                            style.clone(),
                            None,
                        )?);
                    }
                }
                Node::Image(image) => {
                    self.warn(PreviewWarning::ImagesOmitted);
                    self.push_span(&mut spans, &image.alt, &style, link.clone())?;
                }
                Node::Break(_) => self.push_span(&mut spans, " ", &style, link.clone())?,
                Node::Html(_) => self.warn(PreviewWarning::RawHtmlOmitted),
                Node::ImageReference(image) => {
                    self.warn(PreviewWarning::ImagesOmitted);
                    self.push_span(&mut spans, &image.alt, &style, link.clone())?;
                }
                Node::LinkReference(value) => {
                    self.warn(PreviewWarning::UnsupportedMarkdownOmitted);
                    spans.extend(self.inline(&value.children, depth + 1, style.clone(), None)?);
                }
                _ => self.warn(PreviewWarning::UnsupportedMarkdownOmitted),
            }
        }
        Ok(spans)
    }

    fn push_span(
        &mut self,
        spans: &mut Vec<InlineSpan>,
        text: &str,
        style: &InlineStyle,
        link: Option<String>,
    ) -> Result<(), PreviewError> {
        self.add_output(text.len())?;
        if !text.is_empty() {
            spans.push(InlineSpan {
                text: text.to_owned(),
                style: style.clone(),
                link,
            });
        }
        Ok(())
    }

    fn bump(&mut self, depth: usize) -> Result<(), PreviewError> {
        if depth > MAX_DEPTH || self.nodes >= MAX_NODES {
            return Err(PreviewError::ResourceLimit);
        }
        self.nodes += 1;
        Ok(())
    }

    fn add_output(&mut self, count: usize) -> Result<(), PreviewError> {
        self.output_bytes = self
            .output_bytes
            .checked_add(count)
            .ok_or(PreviewError::ResourceLimit)?;
        if self.output_bytes > MAX_OUTPUT_BYTES {
            return Err(PreviewError::ResourceLimit);
        }
        Ok(())
    }

    fn warn(&mut self, warning: PreviewWarning) {
        if !self.document.warnings.contains(&warning) {
            self.document.warnings.push(warning);
        }
    }
}

pub(super) fn parse_markdown(
    source: &str,
    cancelled: &AtomicBool,
) -> Result<MarkdownDocument, PreviewError> {
    if source.len() > MAX_MARKDOWN_BYTES {
        return Err(PreviewError::InputTooLarge);
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }
    let tree = markdown::to_mdast(source, &markdown::ParseOptions::gfm())
        .map_err(|error| PreviewError::MarkdownParse(error.to_string()))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }
    let mut builder = Builder {
        document: MarkdownDocument::default(),
        nodes: 0,
        output_bytes: 0,
    };
    builder.visit(&tree, 0)?;
    Ok(builder.document)
}

fn is_safe_link(url: &str) -> bool {
    if url.len() > 2048 || url.chars().any(char::is_control) {
        return false;
    }
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || (lower.starts_with('#') && !lower[1..].contains('#'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn parses_commonmark_gfm_without_rendering_raw_html_or_remote_images() {
        let document = parse_markdown(
            "# Title\n\n**bold** and [safe](https://example.com)\n\n![remote](https://example.com/x.png)\n\n<script>run()</script>",
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(matches!(
            document.blocks.first(),
            Some(MarkdownBlock::Heading { level: 1, .. })
        ));
        assert!(document.warnings.contains(&PreviewWarning::ImagesOmitted));
        assert!(document.warnings.contains(&PreviewWarning::RawHtmlOmitted));
        let has_link = document.blocks.iter().any(|block| match block {
            MarkdownBlock::Paragraph { spans } => spans.iter().any(|span| span.link.is_some()),
            _ => false,
        });
        assert!(has_link);
    }

    #[test]
    fn blocks_unsafe_link_protocols_and_checks_input_limit() {
        let document =
            parse_markdown("[bad](javascript:alert(1))", &AtomicBool::new(false)).unwrap();
        assert!(
            document
                .warnings
                .contains(&PreviewWarning::UnsafeLinkOmitted)
        );
        assert!(matches!(
            parse_markdown(&"x".repeat(MAX_MARKDOWN_BYTES + 1), &AtomicBool::new(false)),
            Err(PreviewError::InputTooLarge)
        ));
    }

    #[test]
    fn cancellation_is_checked_before_parsing() {
        assert!(matches!(
            parse_markdown("# title", &AtomicBool::new(true)),
            Err(PreviewError::Cancelled)
        ));
    }

    #[test]
    fn renders_unicode_gfm_tables_and_fenced_code_as_blocks() {
        let document = parse_markdown(
            "# Xin chào\n\n| A | B |\n| --- | --- |\n| 1 | 你好 |\n\n```rust\nfn main() {}\n```",
            &AtomicBool::new(false),
        )
        .expect("GFM preview should parse");
        assert!(document.blocks.iter().any(|block| matches!(
            block,
            MarkdownBlock::Heading { spans, .. } if spans.iter().any(|span| span.text == "Xin chào")
        )));
        assert!(
            document
                .blocks
                .iter()
                .any(|block| matches!(block, MarkdownBlock::TableRow { header: true, .. }))
        );
        assert!(document.blocks.iter().any(|block| matches!(
            block,
            MarkdownBlock::Code { language: Some(language), text }
                if language == "rust" && text.contains("fn main")
        )));
    }

    #[test]
    fn empty_markdown_is_a_valid_empty_preview_document() {
        let document = parse_markdown("", &AtomicBool::new(false))
            .expect("empty Markdown should not fail parsing");
        assert!(document.blocks.is_empty());
        assert!(document.warnings.is_empty());
    }
}
