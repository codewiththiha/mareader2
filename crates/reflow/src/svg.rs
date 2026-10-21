//! SVG pictures: sized from the root tag, drawn by resvg when shown.
use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use resvg::tiny_skia;
use resvg::usvg::{self, ImageHrefResolver, ImageKind, fontdb};

use crate::fonts::{BUNDLED, MONO, SANS, SERIF};

/// The images an SVG links, by the reference it writes; nothing else loads.
pub type Links = Vec<(String, Arc<Vec<u8>>)>;

/// How far into a file its root tag is looked for.
const HEAD: usize = 64 << 10;
/// What SVG sizes itself as when it says nothing, as HTML does.
const DEFAULT_SIZE: (f32, f32) = (300.0, 150.0);

/// Whether `data` is SVG markup, plain or gzipped.
pub fn is_svg(data: &[u8]) -> bool {
    data.starts_with(&[0x1f, 0x8b]) || root(&head(data)).is_some()
}

/// The CSS pixel size `data`'s root tag asks for; `None` if not SVG.
pub fn size(data: &[u8]) -> Option<(f32, f32)> {
    if data.starts_with(&[0x1f, 0x8b]) {
        // Gzipped: only a full read knows its size.
        let tree = usvg::Tree::from_data(data, &plain()).ok()?;
        return Some((tree.size().width(), tree.size().height()));
    }
    let text = head(data);
    let tag = root(&text)?;
    let attrs = attributes(tag);
    let get = |name: &str| attrs.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
    let view = get("viewBox").and_then(view_box);
    let (w, h) = (
        get("width").and_then(length),
        get("height").and_then(length),
    );
    let (dw, dh) = DEFAULT_SIZE;
    Some(match (w, h, view) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some((vw, vh))) => (w, w * vh / vw),
        (None, Some(h), Some((vw, vh))) => (h * vw / vh, h),
        (None, None, Some(v)) => v,
        (Some(w), None, None) => (w, dh),
        (None, Some(h), None) => (dw, h),
        (None, None, None) => DEFAULT_SIZE,
    })
}

/// The images `data` refers to by `href`: neither its own parts, data,
/// nor the web.
pub fn links(data: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(data);
    let mut out: Vec<String> = Vec::new();
    for (name, value) in attributes(&text) {
        let value = value.trim();
        let outside = !value.starts_with('#') && !value.starts_with("data:");
        let local = !value.contains("://");
        if name.ends_with("href") && outside && local && !out.iter().any(|o| o == value) {
            out.push(value.to_owned());
        }
    }
    out
}

/// `data` drawn to fit `side` pixels on its long edge, in straight alpha.
pub(crate) fn render(data: &[u8], links: &Links, side: u32) -> Option<image::RgbaImage> {
    let resolve_string = Box::new(|href: &str, _: &usvg::Options| {
        let (_, bytes) = links.iter().find(|(h, _)| h == href.trim())?;
        kind(bytes)
    });
    let opts = usvg::Options {
        image_href_resolver: ImageHrefResolver {
            resolve_data: ImageHrefResolver::default_data_resolver(),
            resolve_string,
        },
        ..plain()
    };
    let tree = usvg::Tree::from_data(&sized(data), &opts).ok()?;
    let (w, h) = (tree.size().width(), tree.size().height());
    let scale = side as f32 / w.max(h).max(1e-3);
    let px = |v: f32| (v * scale).round().clamp(1.0, side.max(1) as f32) as u32;
    let (pw, ph) = (px(w), px(h));
    let mut pixmap = tiny_skia::Pixmap::new(pw, ph)?;
    let fit = tiny_skia::Transform::from_scale(pw as f32 / w, ph as f32 / h);
    resvg::render(&tree, fit, &mut pixmap.as_mut());
    let mut out = Vec::with_capacity(pixmap.pixels().len() * 4);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        out.extend([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    image::RgbaImage::from_raw(pw, ph, out)
}

/// `data` with its root sized as [`size`] reads it, filling its laid box.
fn sized(data: &[u8]) -> Cow<'_, [u8]> {
    let Ok(text) = std::str::from_utf8(data) else {
        return Cow::Borrowed(data);
    };
    let Some(tag) = root(text) else {
        return Cow::Borrowed(data);
    };
    let attrs = attributes(tag);
    let fixed = |name: &str| (attrs.iter()).any(|(n, v)| *n == name && length(v).is_some());
    let Some((w, h)) = size(data).filter(|_| !(fixed("width") && fixed("height"))) else {
        return Cow::Borrowed(data);
    };
    let start = tag.as_ptr() as usize - text.as_ptr() as usize;
    let (Some(before), Some(after)) = (text.get(..start), text.get(start + tag.len()..)) else {
        return Cow::Borrowed(data);
    };
    let mut out = String::with_capacity(text.len() + 48);
    out.push_str(before);
    for (n, v) in attrs
        .iter()
        .filter(|(n, _)| !matches!(*n, "width" | "height"))
    {
        let q = if v.contains('"') { '\'' } else { '"' };
        out.push_str(&format!(" {n}={q}{v}{q}"));
    }
    out.push_str(&format!(" width=\"{w}\" height=\"{h}\""));
    if tag.trim_end().ends_with('/') {
        out.push('/');
    }
    out.push_str(after);
    Cow::Owned(out.into_bytes())
}

/// Options that read nothing outside the markup, with our own faces.
fn plain() -> usvg::Options<'static> {
    usvg::Options {
        resources_dir: None,
        font_family: SERIF.to_owned(),
        image_href_resolver: ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        fontdb: faces(),
        ..usvg::Options::default()
    }
}

/// A linked image as resvg takes it; a linked SVG links nothing more.
fn kind(bytes: &Arc<Vec<u8>>) -> Option<ImageKind> {
    let data = Arc::clone(bytes);
    match bytes.get(..4).unwrap_or_default() {
        [0x89, b'P', b'N', b'G'] => Some(ImageKind::PNG(data)),
        [0xff, 0xd8, ..] => Some(ImageKind::JPEG(data)),
        [b'G', b'I', b'F', _] => Some(ImageKind::GIF(data)),
        [b'R', b'I', b'F', b'F'] => Some(ImageKind::WEBP(data)),
        _ if is_svg(bytes) => usvg::Tree::from_data(bytes, &plain())
            .ok()
            .map(ImageKind::SVG),
        _ => None,
    }
}

/// Our bundled faces, loaded once; SVG text never reads the machine's.
fn faces() -> Arc<fontdb::Database> {
    static DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    let db = DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        for face in BUNDLED {
            db.load_font_source(fontdb::Source::Binary(Arc::new(face)));
        }
        db.set_serif_family(SERIF);
        db.set_sans_serif_family(SANS);
        db.set_monospace_family(MONO);
        db.set_cursive_family(SERIF);
        db.set_fantasy_family(SERIF);
        Arc::new(db)
    });
    Arc::clone(db)
}

fn head(data: &[u8]) -> Cow<'_, str> {
    String::from_utf8_lossy(data.get(..HEAD).unwrap_or(data))
}

/// The root `svg` start tag's inside, prefixed or not, past any prologue.
fn root(text: &str) -> Option<&str> {
    let mut rest = text;
    loop {
        let at = rest.find('<')?;
        rest = rest.get(at + 1..)?;
        if let Some(after) = rest.strip_prefix("!--") {
            rest = after.get(after.find("-->")? + 3..)?;
            continue;
        }
        if rest.starts_with('?') || rest.starts_with('!') {
            continue;
        }
        let name_end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
        let name = rest.get(..name_end)?;
        let local = name.rsplit(':').next().unwrap_or(name);
        if local != "svg" {
            return None;
        }
        return rest.get(name_end..tag_end(rest)?);
    }
}

/// Where the tag `rest` starts ends: its `>`, outside quotes.
fn tag_end(rest: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in rest.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// Every `name="value"` in `text`, in order.
fn attributes(text: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(eq) = rest.find('=') {
        let before = rest.get(..eq).unwrap_or_default().trim_end();
        let name = before
            .rsplit(|c: char| c.is_whitespace() || c == '<')
            .next();
        let after = rest.get(eq + 1..).unwrap_or_default().trim_start();
        let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'');
        let (Some(name), Some(q)) = (name, quote) else {
            rest = after;
            continue;
        };
        let body = after.get(1..).unwrap_or_default();
        let Some(end) = body.find(q) else {
            break;
        };
        out.push((name, body.get(..end).unwrap_or_default()));
        rest = body.get(end + 1..).unwrap_or_default();
    }
    out
}

/// An SVG length in CSS pixels; percentages and `auto` say nothing.
fn length(v: &str) -> Option<f32> {
    const UNITS: [(&str, f32); 8] = [
        ("px", 1.0),
        ("pt", 4.0 / 3.0),
        ("pc", 16.0),
        ("mm", 96.0 / 25.4),
        ("cm", 96.0 / 2.54),
        ("in", 96.0),
        ("em", 16.0),
        ("ex", 8.0),
    ];
    let v = v.trim();
    let unit = UNITS.iter().find(|(u, _)| v.ends_with(u));
    let (number, scale) = match unit {
        Some((u, scale)) => (v.strip_suffix(u).unwrap_or(v), *scale),
        None => (v, 1.0),
    };
    let n: f32 = number.trim().parse().ok()?;
    let px = n * scale;
    (px.is_finite() && px > 0.0).then_some(px)
}

/// A `viewBox`'s width and height, if both are positive.
fn view_box(v: &str) -> Option<(f32, f32)> {
    let n: Vec<f32> = v
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    match n.as_slice() {
        [_, _, w, h] if w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0 => Some((*w, *h)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RING: &str = "<?xml version='1.0'?>\n<!-- art -->\n\
        <svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 40 20' width='8em'>\
        <circle cx='10' cy='10' r='8' fill='black'/></svg>";

    #[test]
    fn the_root_tag_sets_the_size() {
        assert!(is_svg(RING.as_bytes()));
        assert_eq!(size(RING.as_bytes()), Some((128.0, 64.0)));
        let bare = b"<svg xmlns='http://www.w3.org/2000/svg'/>";
        assert_eq!(size(bare), Some(DEFAULT_SIZE));
        let pt = b"<svg:svg width=\"30pt\" height=\"15pt\" xmlns:svg='x'>";
        assert_eq!(size(pt), Some((40.0, 20.0)));
        assert!(!is_svg(b"<html><svg/></html>"));
        assert!(!is_svg(b"\x89PNG"));
    }

    #[test]
    fn only_outside_images_are_links() {
        let doc = b"<svg><use href='#a'/><image xlink:href=\"../i/a.png\"/>\
            <image href='data:image/png;base64,AA'/><image href='https://x/y.png'/>\
            <image href='../i/a.png'/></svg>";
        assert_eq!(links(doc), ["../i/a.png"]);
    }

    #[test]
    fn it_draws_at_the_size_asked() {
        let img = render(RING.as_bytes(), &Vec::new(), 200);
        let Some(img) = img else {
            return assert!(img.is_some(), "render");
        };
        assert_eq!(img.dimensions(), (200, 100));
        // Inside the circle is black; the right half is see-through.
        assert_eq!(img.get_pixel(50, 50).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(180, 50).0[3], 0);
    }

    #[test]
    fn linked_images_come_only_from_the_book() {
        let png = Arc::new(crate::picture::tests::png(4, 4));
        let doc = "<svg xmlns='http://www.w3.org/2000/svg' \
            xmlns:xlink='http://www.w3.org/1999/xlink' width='4' height='4'>\
            <image width='4' height='4' xlink:href='a.png'/></svg>";
        let shown = render(doc.as_bytes(), &vec![("a.png".to_owned(), png)], 4);
        assert!(shown.is_some_and(|i| i.get_pixel(1, 1).0[3] == 255));
        // Unlisted, even an existing file isn't read.
        let shown = render(doc.as_bytes(), &Vec::new(), 4);
        assert!(shown.is_some_and(|i| i.get_pixel(1, 1).0[3] == 0));
    }
}
