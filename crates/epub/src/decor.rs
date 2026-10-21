//! What CSS draws around a box: shadows, gradients and border pictures.

use std::sync::Arc;

use crate::css::{self, Len, Rgb, len, split_top, words};
use crate::generated;

/// A colour and how opaque it is; `None` is the text's colour.
pub(crate) type Tint = (Option<Rgb>, f32);

/// A colour with its alpha: `rgba()`, `#rrggbbaa`, `currentcolor`.
pub(crate) fn tint(v: &str) -> Option<Tint> {
    let v = v.trim().to_ascii_lowercase();
    if v == "currentcolor" {
        return Some((None, 1.0));
    }
    if matches!(v.as_str(), "transparent" | "none") {
        return Some((Some([0; 3]), 0.0));
    }
    let rgb = css::colour(&v)?;
    Some((Some(rgb.unwrap_or([0; 3])), alpha(&v)))
}

/// A colour's alpha, 1 where it sets none.
fn alpha(v: &str) -> f32 {
    if let Some(hex) = v.strip_prefix('#') {
        let digits = |s: &str| u8::from_str_radix(s, 16).ok();
        let a = match hex.len() {
            4 => hex.get(3..4).and_then(digits).map(|d| d * 17),
            8 => hex.get(6..8).and_then(digits),
            _ => None,
        };
        return a.map_or(1.0, |a| f32::from(a) / 255.0);
    }
    let Some(open) = v.find('(') else {
        return 1.0;
    };
    let args = v.get(open + 1..).unwrap_or_default().trim_end_matches(')');
    let parts: Vec<&str> = match args.split_once('/') {
        Some((_, a)) => vec!["", "", "", a],
        None => args.split([',', ' ']).filter(|p| !p.is_empty()).collect(),
    };
    let Some(a) = parts.get(3).map(|a| a.trim()) else {
        return 1.0;
    };
    let value = match a.strip_suffix('%') {
        Some(p) => p.parse::<f32>().map(|p| p / 100.0),
        None => a.parse::<f32>(),
    };
    value.unwrap_or(1.0).clamp(0.0, 1.0)
}

/// One `box-shadow`: offsets, blur and spread as written.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shadow {
    pub(crate) at: [Len; 2],
    pub(crate) blur: Len,
    pub(crate) spread: Len,
    pub(crate) tint: Tint,
    pub(crate) inset: bool,
}

/// `box-shadow`'s list; `none` is empty.
pub(crate) fn shadows(v: &str) -> Arc<[Shadow]> {
    if v.trim() == "none" {
        return Arc::from([]);
    }
    let one = |item: &str| {
        let (mut lengths, mut tint_of, mut inset) = (Vec::new(), None, false);
        for w in words(item) {
            if w == "inset" {
                inset = true;
            } else if let Some(l) = len(w).filter(|_| !w.starts_with('#')) {
                lengths.push(l);
            } else {
                tint_of = tint(w).or(tint_of);
            }
        }
        let at = |i: usize| lengths.get(i).copied().unwrap_or(Len::Px(0.0));
        (lengths.len() >= 2).then(|| Shadow {
            at: [at(0), at(1)],
            blur: at(2),
            spread: at(3),
            tint: tint_of.unwrap_or((None, 1.0)),
            inset,
        })
    };
    split_top(v, ',')
        .into_iter()
        .filter_map(one)
        .take(4)
        .collect()
}

/// A background layer as written: a picture or a gradient.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Layer {
    Url(String),
    Gradient(Gradient),
}

/// A gradient as written; stop places stay lengths.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Gradient {
    pub(crate) radial: bool,
    /// A line's heading in degrees, 180 running down.
    pub(crate) angle: f32,
    pub(crate) at: [f32; 2],
    pub(crate) circle: bool,
    pub(crate) near: bool,
    pub(crate) repeating: bool,
    pub(crate) stops: Vec<(Tint, Option<Len>)>,
}

/// The layers of `background-image` or `background`, case kept.
pub(crate) fn layers(value: &str) -> Vec<Layer> {
    split_top(value, ',')
        .into_iter()
        .filter_map(layer)
        .take(MAX_LAYERS)
        .collect()
}

/// The most background layers a box draws.
const MAX_LAYERS: usize = 6;

/// The picture or gradient in one comma-separated part.
fn layer(part: &str) -> Option<Layer> {
    let lower = part.to_ascii_lowercase();
    let url = lower.find("url(");
    let ramp = lower.find("gradient(");
    match (url, ramp) {
        (Some(u), Some(g)) if u < g => generated::url(part).map(Layer::Url),
        (Some(_), None) => generated::url(part).map(Layer::Url),
        (_, Some(g)) => {
            let before = lower.get(..g)?;
            let name_start = before
                .rfind(|c: char| c.is_whitespace() || matches!(c, ',' | '('))
                .map_or(0, |i| i + 1);
            let name = lower.get(name_start..g)?.trim();
            let body = lower.get(g + "gradient(".len()..)?;
            let inner = body.get(..generated::close_paren(body)?)?;
            gradient(name, inner).map(Layer::Gradient)
        }
        _ => None,
    }
}

/// Whether a background word is a picture or gradient, not a keyword.
pub(crate) fn is_image(w: &str) -> bool {
    let w = w.to_ascii_lowercase();
    w.starts_with("url(") || w.contains("gradient(")
}

/// A gradient from its name's prefix and its arguments.
fn gradient(prefix: &str, inner: &str) -> Option<Gradient> {
    let legacy = prefix.starts_with('-');
    let prefix = prefix.trim_start_matches(['-', 'w', 'e', 'b', 'k', 'i', 't', 'm', 'o', 'z']);
    let prefix = prefix.trim_start_matches('-');
    let repeating = prefix.starts_with("repeating-");
    let kind = prefix.trim_start_matches("repeating-");
    let radial = match kind {
        "linear-" => false,
        "radial-" => true,
        _ => return None,
    };
    let mut parts = split_top(inner, ',');
    let mut g = Gradient {
        radial,
        angle: 180.0,
        at: [0.5, 0.5],
        circle: false,
        near: false,
        repeating,
        stops: Vec::new(),
    };
    let head = parts
        .first()
        .map(|p| p.trim().to_owned())
        .unwrap_or_default();
    let shaped = match radial {
        false => heading(&head, legacy).map(|a| g.angle = a).is_some(),
        true => ring(&head, &mut g),
    };
    if shaped && !parts.is_empty() {
        parts.remove(0);
    }
    for part in parts {
        let (mut tint_of, mut places) = (None, Vec::new());
        for w in words(part) {
            match len(w).filter(|_| !w.starts_with('#')) {
                Some(l) => places.push(l),
                None => tint_of = tint(w).or(tint_of),
            }
        }
        let Some(t) = tint_of else {
            continue;
        };
        match places.as_slice() {
            [] => g.stops.push((t, None)),
            [one] => g.stops.push((t, Some(*one))),
            [a, b, ..] => {
                g.stops.push((t, Some(*a)));
                g.stops.push((t, Some(*b)));
            }
        }
    }
    (g.stops.len() >= 2).then_some(g)
}

/// A linear gradient's heading: an angle, or `to` sides.
fn heading(head: &str, legacy: bool) -> Option<f32> {
    if let Some(a) = angle(head) {
        // Old prefixed syntax measured from east, anticlockwise.
        return Some(if legacy { 90.0 - a } else { a });
    }
    let sides = head.strip_prefix("to ").or(legacy.then_some(head))?;
    let has = |s: &str| sides.split_ascii_whitespace().any(|w| w == s);
    let (up, down, left, right) = (has("top"), has("bottom"), has("left"), has("right"));
    let toward = match (up, down, left, right) {
        (true, _, false, false) => 0.0,
        (true, _, false, true) => 45.0,
        (_, _, false, true) if !down => 90.0,
        (_, true, false, true) => 135.0,
        (_, true, false, false) => 180.0,
        (_, true, true, _) => 225.0,
        (false, false, true, _) => 270.0,
        (true, _, true, _) => 315.0,
        _ => return None,
    };
    // Prefixed sides named where it starts, not where it goes.
    Some(if legacy {
        (toward + 180.0) % 360.0
    } else {
        toward
    })
}

/// `45deg`, `0.25turn`, `1rad`, `100grad`.
fn angle(w: &str) -> Option<f32> {
    let w = w.trim();
    let (number, factor) = if let Some(n) = w.strip_suffix("deg") {
        (n, 1.0)
    } else if let Some(n) = w.strip_suffix("grad") {
        (n, 0.9)
    } else if let Some(n) = w.strip_suffix("rad") {
        (n, 180.0 / std::f32::consts::PI)
    } else {
        (w.strip_suffix("turn")?, 360.0)
    };
    number
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
        .map(|n| n * factor)
}

/// A radial gradient's shape and centre; whether `head` names them.
fn ring(head: &str, g: &mut Gradient) -> bool {
    let (shape, at) = match head.split_once("at ") {
        Some((s, a)) => (s, Some(a)),
        None => (head, None),
    };
    let known = |w: &str| {
        matches!(w, "circle" | "ellipse") || w.starts_with("closest-") || w.starts_with("farthest-")
    };
    let words: Vec<&str> = shape.split_ascii_whitespace().collect();
    let shaped = !words.is_empty() && words.iter().all(|w| known(w) || len(w).is_some());
    if !shaped && at.is_none() {
        return false;
    }
    g.circle = words.contains(&"circle");
    g.near = words.iter().any(|w| w.starts_with("closest-"));
    if let Some(at) = at {
        g.at = css::position(at.trim());
    }
    true
}

/// A side's length in `border-image-width` or `-outset`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) enum Span {
    /// The slice's own size.
    #[default]
    Auto,
    /// Times the border's width.
    Times(f32),
    Len(Len),
}

/// `border-image-slice`: four cuts, percentages or picture pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Slice {
    pub(crate) cut: [f32; 4],
    pub(crate) percent: [bool; 4],
    pub(crate) fill: bool,
}

impl Default for Slice {
    fn default() -> Self {
        Self {
            cut: [100.0; 4],
            percent: [true; 4],
            fill: false,
        }
    }
}

/// `border-image-slice`.
pub(crate) fn slice(v: &str) -> Option<Slice> {
    let fill = v.split_ascii_whitespace().any(|w| w == "fill");
    let cuts: Vec<(f32, bool)> = v
        .split_ascii_whitespace()
        .filter(|w| *w != "fill")
        .filter_map(|w| match w.strip_suffix('%') {
            Some(p) => p.parse::<f32>().ok().map(|p| (p, true)),
            None => w.parse::<f32>().ok().map(|n| (n, false)),
        })
        .collect();
    let all = spread(&cuts)?;
    Some(Slice {
        cut: all.map(|(v, _)| v.max(0.0)),
        percent: all.map(|(_, p)| p),
        fill,
    })
}

/// One to four values given for the four sides, as CSS spreads them.
fn spread<T: Copy>(values: &[T]) -> Option<[T; 4]> {
    let at = |i: usize| values.get(i).copied();
    Some(match values.len() {
        1 => [at(0)?; 4],
        2 => [at(0)?, at(1)?, at(0)?, at(1)?],
        3 => [at(0)?, at(1)?, at(2)?, at(1)?],
        4.. => [at(0)?, at(1)?, at(2)?, at(3)?],
        _ => return None,
    })
}

/// `border-image-width` or `-outset`: numbers, lengths or `auto`.
pub(crate) fn spans(v: &str) -> Option<[Span; 4]> {
    let read = |w: &str| match w {
        "auto" => Some(Span::Auto),
        w => match w.parse::<f32>() {
            Ok(n) => Some(Span::Times(n.max(0.0))),
            Err(_) => len(w).map(Span::Len),
        },
    };
    let values: Vec<Span> = v.split_ascii_whitespace().filter_map(read).collect();
    spread(&values)
}

/// `border-image-repeat`, across then down.
pub(crate) fn tiles(v: &str) -> Option<[mareader_reflow::Tile; 2]> {
    use mareader_reflow::Tile;
    let read = |w: &str| match w {
        "stretch" => Some(Tile::Stretch),
        "repeat" => Some(Tile::Repeat),
        "round" | "space" => Some(Tile::Round),
        _ => None,
    };
    let values: Vec<Tile> = v.split_ascii_whitespace().filter_map(read).collect();
    let first = *values.first()?;
    Some([first, values.get(1).copied().unwrap_or(first)])
}

/// The `border-image` shorthand: source, slice / width / outset, repeat.
pub(crate) struct BorderImage {
    pub(crate) source: Option<String>,
    pub(crate) slice: Slice,
    pub(crate) width: Option<[Span; 4]>,
    pub(crate) outset: [Span; 4],
    pub(crate) tile: [mareader_reflow::Tile; 2],
}

/// `border-image: url(frame.png) 30 fill / 2em / 0 round`.
pub(crate) fn border_image(value: &str) -> BorderImage {
    let source = generated::url(value);
    let lower = value.to_ascii_lowercase();
    // What's left once the picture is taken out.
    let rest = match (lower.find("url("), source.as_ref()) {
        (Some(start), Some(_)) => {
            let after = lower.get(start..).unwrap_or_default();
            let end = after.find(')').map_or(lower.len(), |e| start + e + 1);
            format!(
                "{} {}",
                lower.get(..start).unwrap_or_default(),
                lower.get(end..).unwrap_or_default()
            )
        }
        _ => lower.clone(),
    };
    let tile_words: Vec<&str> = rest
        .split_ascii_whitespace()
        .filter(|w| matches!(*w, "stretch" | "repeat" | "round" | "space"))
        .collect();
    let numbers: String = rest
        .split_ascii_whitespace()
        .filter(|w| !matches!(*w, "stretch" | "repeat" | "round" | "space" | "none"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut sections = numbers.split('/').map(str::trim);
    let slice_of = sections.next().and_then(slice).unwrap_or_default();
    let width = sections.next().and_then(spans);
    let outset = sections.next().and_then(spans);
    BorderImage {
        source,
        slice: slice_of,
        width,
        outset: outset.unwrap_or([Span::Times(0.0); 4]),
        tile: tiles(&tile_words.join(" ")).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_keep_their_alpha() {
        assert_eq!(tint("rgba(0, 0, 0, 0.5)"), Some((Some([0; 3]), 0.5)));
        assert_eq!(tint("rgb(0 0 0 / 25%)"), Some((Some([0; 3]), 0.25)));
        assert_eq!(tint("#00000080").map(|t| (t.1 * 100.0).round()), Some(50.0));
        assert_eq!(tint("currentColor"), Some((None, 1.0)));
        assert_eq!(tint("transparent").map(|t| t.1), Some(0.0));
        assert_eq!(tint("red"), Some((Some([255, 0, 0]), 1.0)));
    }

    #[test]
    fn shadows_read_offsets_blur_spread_and_inset() {
        let s = shadows("2px 3px 4px 1px rgba(0,0,0,.3), inset 0 0 5px #000");
        assert_eq!(s.len(), 2);
        let first = s.first().cloned();
        assert_eq!(
            first.as_ref().map(|s| s.at),
            Some([Len::Px(2.0), Len::Px(3.0)])
        );
        assert_eq!(first.as_ref().map(|s| s.spread), Some(Len::Px(1.0)));
        assert_eq!(first.map(|s| s.tint), Some((Some([0; 3]), 0.3)));
        assert!(s.get(1).is_some_and(|s| s.inset && s.blur == Len::Px(5.0)));
        assert!(shadows("none").is_empty());
    }

    #[test]
    fn gradients_read_heading_shape_and_stops() {
        let l = layers("linear-gradient(to right, transparent, #000 50%, transparent)");
        let half = Some(Some(Len::Percent(50.0)));
        assert!(
            matches!(l.first(), Some(Layer::Gradient(g)) if g.angle == 90.0
            && g.stops.len() == 3 && g.stops.get(1).map(|s| s.1) == half)
        );
        let r = layers("url(a.png), radial-gradient(circle at top left, #fff, #000)");
        assert!(matches!(r.first(), Some(Layer::Url(u)) if u == "a.png"));
        assert!(
            matches!(r.get(1), Some(Layer::Gradient(g)) if g.radial && g.circle
            && g.at == [0.0, 0.0])
        );
        let w = layers("-webkit-linear-gradient(top, #fff, #000)");
        assert!(matches!(w.first(), Some(Layer::Gradient(g)) if g.angle == 180.0));
        let deg = layers("repeating-linear-gradient(45deg, #000 0 2px, #fff 2px 4px)");
        assert!(
            matches!(deg.first(), Some(Layer::Gradient(g)) if g.repeating
            && g.angle == 45.0 && g.stops.len() == 4)
        );
    }

    #[test]
    fn border_images_read_slice_width_outset_and_repeat() {
        let b = border_image("url(\"frame.svg\") 30 27 fill / 2em / 4px round stretch");
        assert_eq!(b.source.as_deref(), Some("frame.svg"));
        assert_eq!(b.slice.cut, [30.0, 27.0, 30.0, 27.0]);
        assert!(b.slice.fill && !b.slice.percent[0]);
        assert_eq!(b.width, Some([Span::Len(Len::Em(2.0)); 4]));
        assert_eq!(b.outset, [Span::Len(Len::Px(4.0)); 4]);
        use mareader_reflow::Tile;
        assert_eq!(b.tile, [Tile::Round, Tile::Stretch]);
        let plain = border_image("url(b.png) 10% repeat");
        assert_eq!(plain.slice.cut, [10.0; 4]);
        assert_eq!(plain.tile, [Tile::Repeat; 2]);
        assert_eq!(slice("33.3%").map(|s| s.percent), Some([true; 4]));
    }
}
