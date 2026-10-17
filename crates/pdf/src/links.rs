//! A page's link areas: a jump within the file, or a web address.

use hayro::hayro_interpret::TransformExt as _;
use hayro::hayro_syntax::object::dict::keys::{A, ANNOTS, N, RECT, S, SUBTYPE, URI};
use hayro::hayro_syntax::object::{Array, Dict, Name, Rect, String as PdfString};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::Point;

use crate::dest::Targets;
use crate::text::Bounds;

/// Links kept per page; more is a broken or hostile file.
pub const MAX_LINKS: usize = 2_000;
/// Only these open outside the app; `javascript:` and `file:` never do.
const SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// A 0-based page in this file.
    Page(usize),
    /// A book's footnote: what it says, shown in place, and its page.
    Note { page: usize, text: String },
    /// An address for the browser or mail app, scheme already checked.
    Web(String),
}

/// A clickable area on a page, in page points.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub bounds: Bounds,
    pub target: LinkTarget,
}

/// Reads the `/Link` annotations whose target is usable.
pub(crate) fn read(page: &Page<'_>, index: usize, targets: &Targets<'_>) -> Vec<Link> {
    let Some(annots) = page.raw().get::<Array<'_>>(ANNOTS) else {
        return Vec::new();
    };
    let to_page = page.initial_transform(true).to_kurbo();
    annots
        .iter::<Dict<'_>>()
        .filter(|a| {
            a.get::<Name<'_>>(SUBTYPE)
                .is_some_and(|s| s.as_ref() == b"Link")
        })
        .filter_map(|a| {
            let r = a.get::<Rect>(RECT)?;
            let corners = [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)];
            let bounds = Bounds::around(corners.map(|(x, y)| to_page * Point::new(x, y)))?;
            let target = target(&a, index, targets)?;
            Some(Link { bounds, target })
        })
        .take(MAX_LINKS)
        .collect()
}

/// A destination, a GoTo, a web address, or a named page turn.
fn target(annot: &Dict<'_>, index: usize, targets: &Targets<'_>) -> Option<LinkTarget> {
    if let Some(page) = targets.of_node(annot) {
        return Some(LinkTarget::Page(page));
    }
    let action = annot.get::<Dict<'_>>(A)?;
    let kind = action.get::<Name<'_>>(S)?;
    match kind.as_ref() {
        b"URI" => web_address(action.get::<PdfString<'_>>(URI)?.as_ref()).map(LinkTarget::Web),
        b"Named" => {
            let last = targets.count().checked_sub(1)?;
            let page = match action.get::<Name<'_>>(N)?.as_ref() {
                b"NextPage" => (index + 1).min(last),
                b"PrevPage" => index.saturating_sub(1),
                b"FirstPage" => 0,
                b"LastPage" => last,
                _ => return None,
            };
            Some(LinkTarget::Page(page))
        }
        _ => None,
    }
}

/// The address if its scheme is one mareader opens, else nothing.
pub fn web_address(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let allowed = SCHEMES
        .iter()
        .any(|s| lower.starts_with(s) && lower.len() > s.len());
    (allowed && !text.chars().any(char::is_control)).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, OpenError, sample};
    use std::sync::Arc;

    type Out = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn only_web_and_mail_schemes_are_followed() {
        assert_eq!(
            web_address(b" https://example.com/a?b=1 ").as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert!(web_address(b"HTTP://EXAMPLE.COM").is_some());
        assert!(web_address(b"mailto:me@example.com").is_some());
        for bad in [
            &b"javascript:alert(1)"[..],
            b"file:///etc/passwd",
            b"https://",
            b"ftp://x",
            b"https://a\nb",
            b"",
        ] {
            assert_eq!(web_address(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn link_annotations_resolve_to_pages_and_checked_addresses() -> Out {
        let page = |p: usize| format!("{} 0 R", 6 + 2 * p);
        let annots = [
            format!("<< /Subtype /Link /Rect [72 700 172 720] /Dest [{} /Fit] >>", page(2)),
            "<< /Subtype /Link /Rect [72 600 172 620] /A << /S /URI /URI (https://example.com) >> >>".to_string(),
            "<< /Subtype /Link /Rect [0 0 10 10] /A << /S /URI /URI (javascript:x) >> >>".to_string(),
            "<< /Subtype /Link /Rect [0 0 10 10] /A << /S /Named /N /NextPage >> >>".to_string(),
            "<< /Subtype /Text /Rect [0 0 10 10] /Dest [6 0 R /Fit] >>".to_string(),
            "<< /Subtype /Link /Dest [6 0 R /Fit] >>".to_string(),
        ];
        let bytes = sample::with_annots(4, 0, &annots);
        let doc = Document::open(Arc::new(bytes)).map_err(|e: OpenError| e.to_string())?;
        let layer = doc.text_layer(0)?;
        let targets: Vec<_> = layer.links.iter().map(|l| l.target.clone()).collect();
        assert_eq!(
            targets,
            [
                LinkTarget::Page(2),
                LinkTarget::Web("https://example.com".into()),
                LinkTarget::Page(1),
            ]
        );
        // Flipped to the page's top-left origin: 792 - 720 = 72.
        let first = layer.links.first().map(|l| l.bounds);
        assert_eq!(
            first,
            Some(Bounds {
                x0: 72.0,
                y0: 72.0,
                x1: 172.0,
                y1: 92.0
            })
        );
        Ok(())
    }
}
