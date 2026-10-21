//! Audio and video cards, picked picture sources and image maps.

use std::cell::RefCell;
use std::collections::HashMap;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use mareader_reflow::Length;

/// What a file inside the book that plays is, to hand to a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub data: Vec<u8>,
    /// The name players know its type by, from its declared type.
    pub extension: &'static str,
}

/// Namespaces an `epub:case` may require that the reader draws.
const DRAWN: [&str; 3] = [
    "http://www.w3.org/1998/Math/MathML",
    "http://www.w3.org/2000/svg",
    "http://www.w3.org/1999/xhtml",
];

/// Image maps and their areas a chapter may hold.
const MAX_MAPS: usize = 64;
const MAX_AREAS: usize = 256;
/// Manifest fallbacks tried for one picture.
pub(crate) const MAX_FALLBACKS: usize = 8;

/// The name a card's picture links its poster by.
pub(crate) const POSTER: &str = "poster";

/// A `<video>` or `<audio>` read so far, drawn as a card at its end.
#[derive(Debug, Clone, Default)]
pub(crate) struct Pending {
    /// Its place in the stack of open elements.
    pub(crate) depth: usize,
    pub(crate) video: bool,
    pub(crate) src: Option<String>,
    pub(crate) poster: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) width: Option<Length>,
    /// Its `<track>`s of subtitles or captions, as written.
    pub(crate) tracks: Vec<TrackTag>,
}

/// A `<track>` as its card read it, its file not yet found.
#[derive(Debug, Clone, Default)]
pub(crate) struct TrackTag {
    pub(crate) src: String,
    pub(crate) language: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) default: bool,
}

/// What a card's clip is to this reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clip {
    /// In the book with a type players know, or on the web.
    Plays,
    /// In the book, but not something a player opens.
    Held,
    Gone,
}

/// An `<area>` of an image map: its bounds in image pixels, or all of it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MapArea {
    /// Left, top, right, bottom; `None` covers the whole picture.
    pub(crate) bounds: Option<[f32; 4]>,
    pub(crate) href: String,
}

/// Whether an `epub:case` requiring `ns` can be shown.
pub(crate) fn drawn_namespace(ns: &str) -> bool {
    DRAWN.contains(&ns.trim())
}

/// `media_type` without parameters, lowercased.
fn bare(media_type: &str) -> String {
    let t = media_type.split(';').next().unwrap_or_default();
    t.trim().to_ascii_lowercase()
}

/// Image types pictures decode; others need a fallback.
pub(crate) fn decodes(media_type: &str) -> bool {
    matches!(
        bare(media_type).as_str(),
        "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/webp" | "image/svg+xml"
    )
}

/// Whether `media_type` is sound or moving pictures.
pub(crate) fn plays(media_type: &str) -> bool {
    let t = bare(media_type);
    t.starts_with("audio/") || t.starts_with("video/")
}

/// The file extension players know `media_type` by; none for others.
pub(crate) fn extension(media_type: &str) -> Option<&'static str> {
    Some(match bare(media_type).as_str() {
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => "m4a",
        "audio/aac" | "audio/aacp" => "aac",
        "audio/ogg" | "audio/vorbis" => "ogg",
        "audio/opus" => "opus",
        "audio/wav" | "audio/wave" | "audio/x-wav" => "wav",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/webm" => "weba",
        "video/mp4" => "mp4",
        "video/x-m4v" => "m4v",
        "video/webm" => "webm",
        "video/ogg" => "ogv",
        "video/quicktime" => "mov",
        "video/x-matroska" => "mkv",
        "video/3gpp" => "3gp",
        "video/mpeg" => "mpg",
        _ => return None,
    })
}

/// A `srcset`'s largest candidate, by width or density.
pub(crate) fn best_candidate(srcset: &str) -> Option<String> {
    let srcset = srcset.trim();
    // A data URL's commas are its own, so it is one candidate.
    if srcset.starts_with("data:") {
        return srcset.split_whitespace().next().map(str::to_owned);
    }
    let rank = |descriptor: Option<&str>| {
        let d = descriptor.unwrap_or("1x");
        let (number, unit) = d.split_at(d.len().saturating_sub(1));
        let n = number.parse::<f32>().unwrap_or(1.0);
        match unit {
            "w" => n,
            _ => n * 1000.0,
        }
    };
    let mut best: Option<(f32, &str)> = None;
    for candidate in srcset.split(',') {
        let mut words = candidate.split_whitespace();
        let Some(url) = words.next() else {
            continue;
        };
        let r = rank(words.next());
        if best.is_none_or(|(b, _)| r > b) {
            best = Some((r, url));
        }
    }
    best.map(|(_, url)| url.to_owned())
}

/// What a card calls its file: its title, else its file's name.
pub(crate) fn label(title: Option<&str>, src: &str) -> String {
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        return t.chars().take(80).collect();
    }
    let path = src.split(['?', '#']).next().unwrap_or_default();
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path);
    let name = crate::href::percent_decode(name);
    name.chars().take(80).collect()
}

/// A drawn stand-in for a video with no poster: a frame and a play mark.
pub(crate) fn video_card() -> Vec<u8> {
    br##"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180" viewBox="0 0 320 180">
<rect x="2" y="2" width="316" height="176" rx="12" fill="none" stroke="#222" stroke-width="3"/>
<circle cx="160" cy="82" r="34" fill="none" stroke="#222" stroke-width="4"/>
<path d="M149 64 L181 82 L149 100 Z" fill="#222"/>
<rect x="18" y="150" width="284" height="4" rx="2" fill="#222" fill-opacity="0.35"/>
<rect x="18" y="150" width="64" height="4" rx="2" fill="#222"/>
</svg>"##
        .to_vec()
}

/// A drawn player strip for audio: a play mark, a track and a speaker.
pub(crate) fn audio_card() -> Vec<u8> {
    br##"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="52" viewBox="0 0 320 52">
<rect x="2" y="2" width="316" height="48" rx="24" fill="none" stroke="#222" stroke-width="3"/>
<circle cx="27" cy="26" r="15" fill="none" stroke="#222" stroke-width="3"/>
<path d="M22 18 L35 26 L22 34 Z" fill="#222"/>
<rect x="54" y="24" width="200" height="4" rx="2" fill="#222" fill-opacity="0.35"/>
<rect x="54" y="24" width="48" height="4" rx="2" fill="#222"/>
<path d="M270 21 H276 L284 14 V38 L276 31 H270 Z" fill="#222"/>
<path d="M290 18 Q296 26 290 34 M296 13 Q306 26 296 39" fill="none" stroke="#222" stroke-width="2.5"/>
</svg>"##
        .to_vec()
}

/// A poster `width` × `height` with a play mark over its middle.
pub(crate) fn poster_card(width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let r = w.min(h) * 0.13;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let line = (r * 0.09).max(1.0);
    let (left, top, bottom, tip) = (cx - r * 0.32, cy - r * 0.45, cy + r * 0.45, cx + r * 0.5);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\
         <image xlink:href=\"{POSTER}\" x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" \
         preserveAspectRatio=\"none\"/>\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"#000\" fill-opacity=\"0.55\" \
         stroke=\"#fff\" stroke-width=\"{line}\"/>\
         <path d=\"M{left} {top} L{tip} {cy} L{left} {bottom} Z\" fill=\"#fff\"/></svg>"
    )
    .into_bytes()
}

/// The numbers of an area's `coords`, apart by commas or spaces.
fn numbers(coords: &str) -> Vec<f32> {
    coords
        .split([',', ' ', '\t', '\n'])
        .filter_map(|n| n.trim().parse::<f32>().ok())
        .filter(|n| n.is_finite())
        .take(512)
        .collect()
}

/// An area's bounds from its `shape` and `coords`; `None` if unusable.
fn bounds(shape: &str, coords: &str) -> Option<Option<[f32; 4]>> {
    let n = numbers(coords);
    let span = |xs: &mut dyn Iterator<Item = f32>| {
        xs.fold(None, |acc: Option<(f32, f32)>, v| match acc {
            Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
            None => Some((v, v)),
        })
    };
    match shape.trim().to_ascii_lowercase().as_str() {
        "default" => Some(None),
        "circle" | "circ" => match n.as_slice() {
            [x, y, r, ..] if *r > 0.0 => Some(Some([x - r, y - r, x + r, y + r])),
            _ => None,
        },
        "poly" | "polygon" if n.len() >= 6 => {
            let (x0, x1) = span(&mut n.iter().step_by(2).copied())?;
            let (y0, y1) = span(&mut n.iter().skip(1).step_by(2).copied())?;
            Some(Some([x0, y0, x1, y1]))
        }
        "" | "rect" | "rectangle" => match n.as_slice() {
            [a, b, c, d, ..] => Some(Some([a.min(*c), b.min(*d), a.max(*c), b.max(*d)])),
            _ => None,
        },
        _ => None,
    }
}

/// The chapter's image maps by name, read ahead of the pictures using them.
pub(crate) fn image_maps(xhtml: &str) -> Maps {
    if !xhtml.contains("<map") && !xhtml.contains("<MAP") {
        return HashMap::new();
    }
    let sink = MapScan(RefCell::default());
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(xhtml));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.0.into_inner().1
}

fn attr<'t>(tag: &'t Tag, name: &str) -> Option<&'t str> {
    let found = tag.attrs.iter().find(|a| &*a.name.local == name);
    found.map(|a| &*a.value)
}

/// Image maps by name.
type Maps = HashMap<String, Vec<MapArea>>;

/// The map being read, and those read.
struct MapScan(RefCell<(Option<String>, Maps)>);

impl TokenSink for MapScan {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let Token::TagToken(tag) = token else {
            return TokenSinkResult::Continue;
        };
        let mut scan = self.0.borrow_mut();
        let (open, maps) = &mut *scan;
        match (tag.kind, &*tag.name) {
            (TagKind::StartTag, "map") => {
                let name = attr(&tag, "name").or_else(|| attr(&tag, "id"));
                *open = name
                    .map(|n| n.trim().to_owned())
                    .filter(|_| maps.len() < MAX_MAPS);
            }
            (TagKind::EndTag, "map") => *open = None,
            (TagKind::StartTag, "area") => {
                let Some(name) = open.as_ref() else {
                    return TokenSinkResult::Continue;
                };
                let href = attr(&tag, "href").map(str::trim).filter(|h| !h.is_empty());
                let shape = attr(&tag, "shape").unwrap_or_default();
                let found = bounds(shape, attr(&tag, "coords").unwrap_or_default());
                let areas = maps.entry(name.clone()).or_default();
                if let (Some(href), Some(bounds)) = (href, found)
                    && areas.len() < MAX_AREAS
                {
                    let href = href.to_owned();
                    areas.push(MapArea { bounds, href });
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_largest_candidate_wins() {
        let best = |s: &str| best_candidate(s);
        assert_eq!(
            best("a.png 480w, b.png 1200w, c.png 800w").as_deref(),
            Some("b.png")
        );
        assert_eq!(best("a.png, b.png 2x").as_deref(), Some("b.png"));
        assert_eq!(best("only.png").as_deref(), Some("only.png"));
        assert_eq!(
            best("data:image/png;base64,AAAA 1x").as_deref(),
            Some("data:image/png;base64,AAAA")
        );
        assert_eq!(best("  ,  "), None);
    }

    #[test]
    fn players_know_files_by_their_declared_type() {
        assert_eq!(extension("audio/mpeg"), Some("mp3"));
        assert_eq!(extension("Video/MP4; codecs=avc1"), Some("mp4"));
        assert_eq!(extension("application/x-msdownload"), None);
        assert_eq!(extension("text/html"), None);
        assert!(plays("audio/ogg") && plays("video/webm") && !plays("image/png"));
    }

    #[test]
    fn areas_are_bounded_by_their_shapes() {
        assert_eq!(
            bounds("rect", "30,40,10,20"),
            Some(Some([10.0, 20.0, 30.0, 40.0]))
        );
        assert_eq!(
            bounds("circle", "50 50 10"),
            Some(Some([40.0, 40.0, 60.0, 60.0]))
        );
        assert_eq!(
            bounds("poly", "0,10, 20,0, 40,30"),
            Some(Some([0.0, 0.0, 40.0, 30.0]))
        );
        assert_eq!(bounds("default", ""), Some(None));
        assert_eq!(bounds("rect", "1,2"), None);
        assert_eq!(bounds("star", "1,2,3,4"), None);
    }

    #[test]
    fn maps_are_read_ahead_by_name() {
        let maps = image_maps(
            "<p><img usemap='#m' src='a.png'/></p><map name='m'>\
             <area shape='rect' coords='0,0,10,10' href='a.xhtml'/>\
             <area shape='rect' coords='0,0,10,10'/>\
             <area shape='default' href='#top'/></map>",
        );
        let areas = maps.get("m").map(Vec::as_slice).unwrap_or_default();
        assert_eq!(areas.len(), 2, "{areas:?}");
        assert_eq!(areas[0].bounds, Some([0.0, 0.0, 10.0, 10.0]));
        assert_eq!(areas[1].bounds, None);
        assert!(image_maps("<p>no maps</p>").is_empty());
    }

    #[test]
    fn cards_are_labelled_by_title_or_file() {
        assert_eq!(label(Some(" Waves "), "a/b.mp4"), "Waves");
        assert_eq!(
            label(None, "../media/ocean%20waves.mp3?x=1"),
            "ocean waves.mp3"
        );
        assert!(drawn_namespace("http://www.w3.org/1998/Math/MathML"));
        assert!(!drawn_namespace("http://www.xml-cml.org/schema"));
    }
}
