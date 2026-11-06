use std::sync::atomic::{AtomicBool, Ordering};

use crate::PreviewError;

const MAX_SVG_BYTES: usize = 4 * 1024 * 1024;
const MAX_PATH_NODES: usize = 50_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SvgPolicy {
    pub max_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_raster_pixels: u64,
    pub max_dimension: u32,
}

impl Default for SvgPolicy {
    fn default() -> Self {
        Self {
            max_bytes: MAX_SVG_BYTES,
            max_nodes: MAX_PATH_NODES,
            max_depth: 128,
            max_raster_pixels: 8_000_000,
            max_dimension: 8192,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SvgDocument {
    pub width: u32,
    pub height: u32,
    /// PNG bytes produced by resvg; no SVG input is passed to the UI renderer.
    pub png: Vec<u8>,
}

pub(super) fn render_svg(
    source: &str,
    policy: &SvgPolicy,
    cancelled: &AtomicBool,
) -> Result<SvgDocument, PreviewError> {
    if source.len() > policy.max_bytes {
        return Err(PreviewError::InputTooLarge);
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }
    validate_xml(source, policy)?;

    let options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(source, &options)
        .map_err(|error| PreviewError::SvgParse(error.to_string()))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }

    let size = tree.size();
    let (width, height) = (size.width().ceil(), size.height().ceil());
    if !width.is_finite() || !height.is_finite() || width < 1. || height < 1. {
        return Err(PreviewError::InvalidSvgSize);
    }
    let (width, height) = (width as u32, height as u32);
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(PreviewError::InvalidSvgSize)?;
    if width > policy.max_dimension
        || height > policy.max_dimension
        || pixels > policy.max_raster_pixels
    {
        return Err(PreviewError::InvalidSvgSize);
    }

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(PreviewError::InvalidSvgSize)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    if cancelled.load(Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }
    let png = pixmap
        .encode_png()
        .map_err(|error| PreviewError::SvgRender(error.to_string()))?;
    Ok(SvgDocument { width, height, png })
}

fn validate_xml(source: &str, policy: &SvgPolicy) -> Result<(), PreviewError> {
    let upper = source.to_ascii_uppercase();
    if upper.contains("<!DOCTYPE")
        || upper.contains("<!ENTITY")
        || upper.contains("<?XML-STYLESHEET")
    {
        return Err(PreviewError::UnsafeSvg);
    }
    let document = roxmltree::Document::parse(source)
        .map_err(|error| PreviewError::SvgParse(error.to_string()))?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return Err(PreviewError::SvgParse("root element is not <svg>".into()));
    }

    let mut pending = vec![(root, 0usize)];
    let mut count = 0usize;
    while let Some((node, depth)) = pending.pop() {
        count = count.checked_add(1).ok_or(PreviewError::ResourceLimit)?;
        if count > policy.max_nodes || depth > policy.max_depth {
            return Err(PreviewError::ResourceLimit);
        }
        if node.is_element() {
            let name = node.tag_name().name();
            if matches!(
                name,
                "script"
                    | "foreignObject"
                    | "image"
                    | "use"
                    | "style"
                    | "animate"
                    | "animateMotion"
                    | "animateTransform"
                    | "set"
            ) {
                return Err(PreviewError::UnsafeSvg);
            }
            for attribute in node.attributes() {
                let name = attribute.name().to_ascii_lowercase();
                let value = attribute.value();
                if name == "href" || name.starts_with("on") || value.contains('\\') {
                    return Err(PreviewError::UnsafeSvg);
                }
                if value.to_ascii_lowercase().contains("url") && !is_local_paint_reference(value) {
                    return Err(PreviewError::UnsafeSvg);
                }
            }
        }
        pending.extend(
            node.children()
                .filter(|child| child.is_element())
                .map(|child| (child, depth + 1)),
        );
    }
    Ok(())
}

fn is_local_paint_reference(value: &str) -> bool {
    let Some(id) = value
        .trim()
        .strip_prefix("url(#")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_a_bounded_native_svg() {
        let result = render_svg(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16"><rect width="32" height="16" fill="#ff0000"/></svg>"##,
            &SvgPolicy::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!((result.width, result.height), (32, 16));
        assert!(result.png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn rejects_active_external_and_entity_content() {
        for svg in [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://example.com/a.png"/></svg>"#,
            r#"<!DOCTYPE svg [<!ENTITY x "boom">]><svg xmlns="http://www.w3.org/2000/svg"/>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect style="fill:url(https://example.com/a)"/></svg>"#,
        ] {
            assert!(matches!(
                render_svg(svg, &SvgPolicy::default(), &AtomicBool::new(false)),
                Err(PreviewError::UnsafeSvg)
            ));
        }
    }

    #[test]
    fn enforces_pixel_and_depth_budgets_and_cancellation() {
        let oversized = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10000" height="10000"/>"#;
        assert!(matches!(
            render_svg(oversized, &SvgPolicy::default(), &AtomicBool::new(false)),
            Err(PreviewError::InvalidSvgSize)
        ));
        let policy = SvgPolicy {
            max_depth: 1,
            ..SvgPolicy::default()
        };
        let nested =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect width="2" height="2"/></g></svg>"#;
        assert!(matches!(
            render_svg(nested, &policy, &AtomicBool::new(false)),
            Err(PreviewError::ResourceLimit)
        ));
        assert!(matches!(
            render_svg("<svg/>", &SvgPolicy::default(), &AtomicBool::new(true)),
            Err(PreviewError::Cancelled)
        ));
    }
}
