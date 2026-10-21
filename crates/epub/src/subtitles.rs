//! Subtitle tracks of a book's clips, read as timed lines of text.

use mareader_reflow::{Align, Block, Content, Emphasis, Role, Text};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::overlay::clock;

/// Tracks kept per clip; later ones are ignored.
pub const MAX_TRACKS: usize = 8;
/// Clips with tracks kept per book.
pub const MAX_CARDS: usize = 1024;
/// Lines read from one track, and the bytes a track may take.
pub const MAX_CUES: usize = 4000;
pub const MAX_TRACK: usize = 4 << 20;
/// Characters kept of one line.
const MAX_CUE_TEXT: usize = 2000;
/// A line's time, in a grey the theme turns to its own.
const STAMP: [u8; 3] = [112, 112, 112];
/// Frames a second in a TTML `hh:mm:ss:ff` clock without its own rate.
const TTML_FRAMES: f64 = 30.0;

/// A subtitle or caption track a clip names, as `<track>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Its file in the book.
    pub path: String,
    /// Its `srclang` tag, if it has one.
    pub language: Option<String>,
    pub label: Option<String>,
    /// Marked `default`: the one to show when no language fits.
    pub default: bool,
}

/// A line of subtitles and when it shows, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// A track's file to copy beside its clip, for a player to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtitle {
    pub language: Option<String>,
    /// `vtt`, `srt` or `ttml`, by what it holds.
    pub extension: &'static str,
    pub data: Vec<u8>,
}

/// What a reader wants a transcript in.
#[derive(Debug, Clone, Copy)]
pub struct Wanted<'a> {
    /// Language tags, most wanted first.
    pub languages: &'a [String],
    /// A tag's name, as "French"; none shows the track's own words.
    pub name: fn(&str) -> Option<&'static str>,
}

/// A clip's tracks and the links that show its transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Card {
    /// The clip in the book, if it plays.
    pub(crate) clip: Option<String>,
    pub(crate) tracks: Vec<Track>,
    /// The block of its "Show transcript" link; none if it has no room.
    pub(crate) caption: Option<usize>,
    /// That link's target, and one per track that picks it.
    pub(crate) toggle: u32,
    pub(crate) picks: Vec<u32>,
}

/// How a track's file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    WebVtt,
    SubRip,
    Ttml,
}

impl Format {
    /// Told by what the file starts with, never by its name.
    pub(crate) fn of(data: &[u8]) -> Self {
        let head = String::from_utf8_lossy(data.get(..512).unwrap_or(data));
        let head = head.trim_start_matches('\u{feff}').trim_start();
        if head.starts_with("WEBVTT") {
            Self::WebVtt
        } else if head.starts_with('<') && head.contains("tt") {
            Self::Ttml
        } else {
            Self::SubRip
        }
    }

    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::WebVtt => "vtt",
            Self::SubRip => "srt",
            Self::Ttml => "ttml",
        }
    }
}

/// `data`'s lines in time order, each on one line of text.
pub fn cues(data: &[u8]) -> Vec<Cue> {
    let data = data.get(..MAX_TRACK).unwrap_or(data);
    let mut out = match Format::of(data) {
        Format::Ttml => ttml(data),
        _ => blocks(&String::from_utf8_lossy(data)),
    };
    out.retain(|c| !c.text.is_empty());
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    // Rolling captions repeat a line as it moves up; once is enough.
    out.dedup_by(|b, a| a.text == b.text && b.start <= a.end + 0.5);
    out
}

/// WebVTT and SubRip: blank-line separated blocks, a timing line in each.
fn blocks(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut out = Vec::new();
    for block in text.replace('\r', "\n").split("\n\n") {
        if out.len() >= MAX_CUES {
            break;
        }
        let mut lines = block.lines().skip_while(|l| !l.contains("-->"));
        let Some((start, end)) = lines.next().and_then(timing) else {
            continue;
        };
        let words: Vec<String> = lines.map(cue_text).filter(|l| !l.is_empty()).collect();
        out.push(Cue {
            start,
            end,
            text: shorten(words.join(" ")),
        });
    }
    out
}

/// `00:01.000 --> 00:04.000 align:start`: its two times.
fn timing(line: &str) -> Option<(f64, f64)> {
    let (from, to) = line.split_once("-->")?;
    let time = |s: &str| clock(&s.trim().replace(',', "."));
    let start = time(from)?;
    let end = time(to.split_whitespace().next()?)?;
    Some((start, end.max(start)))
}

/// A cue's line without its tags; a voice tag names who speaks.
fn cue_text(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line.trim();
    while let Some(open) = rest.find('<') {
        out.push_str(rest.get(..open).unwrap_or_default());
        let after = rest.get(open + 1..).unwrap_or_default();
        let Some(close) = after.find('>') else {
            rest = "";
            break;
        };
        let tag = after.get(..close).unwrap_or_default();
        if let Some(voice) = tag.strip_prefix('v').filter(|v| v.starts_with([' ', '.'])) {
            let name = voice.split_once(' ').map_or("", |(_, n)| n.trim());
            if !name.is_empty() {
                out.push_str(name);
                out.push_str(": ");
            }
        }
        rest = after.get(close + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    let out = entities(&out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The character references WebVTT names, and numbered ones.
fn entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let named = [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&apos;", "'"),
        ("&nbsp;", " "),
        ("&lrm;", ""),
        ("&rlm;", ""),
    ];
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let tail = rest.get(at..).unwrap_or_default();
        if let Some((from, to)) = named.iter().find(|(n, _)| tail.starts_with(n)) {
            out.push_str(to);
            rest = tail.get(from.len()..).unwrap_or_default();
            continue;
        }
        let numbered = tail.strip_prefix("&#").and_then(|t| {
            let (digits, after) = t.split_once(';')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            Some((char::from_u32(code)?, after))
        });
        match numbered {
            Some((c, after)) => {
                out.push(c);
                rest = after;
            }
            None => {
                out.push('&');
                rest = tail.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

/// TTML: each `<p>` with its `begin`, and `end` or `dur`.
fn ttml(data: &[u8]) -> Vec<Cue> {
    let text = String::from_utf8_lossy(data);
    let mut reader = Reader::from_str(&text);
    let mut out = Vec::new();
    // The `<p>` open now: its times, its text, and how deep inside it.
    let mut open: Option<(f64, f64, String, usize)> = None;
    while out.len() < MAX_CUES {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(e) => e,
        };
        match event {
            Event::Start(e) if e.local_name().as_ref() == "p" && open.is_none() => {
                let time = |n: &str| attribute(&e, n).and_then(|v| ttml_clock(&v));
                let start = time("begin").unwrap_or(0.0);
                let end = time("end").or_else(|| Some(start + time("dur")?));
                open = Some((start, end.unwrap_or(start), String::new(), 0));
            }
            Event::Start(_) => {
                if let Some(p) = open.as_mut() {
                    p.3 += 1;
                }
            }
            Event::Empty(e) if e.local_name().as_ref() == "br" => {
                if let Some(p) = open.as_mut() {
                    p.2.push(' ');
                }
            }
            Event::Text(t) => {
                if let Some(p) = open.as_mut() {
                    p.2.push_str(&entities(&t));
                }
            }
            Event::GeneralRef(r) => {
                if let Some(p) = open.as_mut() {
                    let raw = format!("&{};", &*r);
                    p.2.push_str(&entities(&raw));
                }
            }
            Event::End(_) => match open.as_mut() {
                Some(p) if p.3 > 0 => p.3 -= 1,
                Some(_) => {
                    if let Some((start, end, text, _)) = open.take() {
                        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                        let end = end.max(start);
                        let text = shorten(text);
                        out.push(Cue { start, end, text });
                    }
                }
                None => {}
            },
            _ => {}
        }
    }
    out
}

/// A TTML time: a clock with frames, or any SMIL-like value.
fn ttml_clock(value: &str) -> Option<f64> {
    let parts: Vec<&str> = value.trim().split(':').collect();
    if let [h, m, s, f] = parts.as_slice() {
        let n = |v: &str| v.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0);
        return Some(n(h)? * 3600.0 + n(m)? * 60.0 + n(s)? + n(f)? / TTML_FRAMES);
    }
    clock(value)
}

/// Attribute `name`'s value by its local name.
fn attribute(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    let mut attributes = e.attributes();
    attributes.with_checks(false);
    let a = attributes
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)?;
    Some(entities(&a.value))
}

fn shorten(mut text: String) -> String {
    if let Some((at, _)) = text.char_indices().nth(MAX_CUE_TEXT) {
        text.truncate(at);
    }
    text
}

/// The track to show: a wanted language, else the default, else the first.
pub fn pick(tracks: &[Track], wanted: &[String]) -> Option<usize> {
    let primary = |t: &str| {
        let t = t.trim().to_ascii_lowercase().replace('_', "-");
        let head = t.split('-').next().unwrap_or_default().to_owned();
        (t, head)
    };
    for want in wanted {
        let (tag, head) = primary(want);
        if head.is_empty() {
            continue;
        }
        let tags: Vec<Option<(String, String)>> = (tracks.iter())
            .map(|t| t.language.as_deref().map(primary))
            .collect();
        let exact = tags
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| t.0 == tag));
        let near = || {
            tags.iter()
                .position(|t| t.as_ref().is_some_and(|t| t.1 == head))
        };
        if let Some(i) = exact.or_else(near) {
            return Some(i);
        }
    }
    let default = tracks.iter().position(|t| t.default);
    default.or((!tracks.is_empty()).then_some(0))
}

/// What a track is called: its label, its language's name, or its tag.
pub fn track_name(track: &Track, index: usize, name: fn(&str) -> Option<&'static str>) -> String {
    if let Some(label) = track
        .label
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        return label.to_owned();
    }
    let Some(tag) = track
        .language
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return format!("Track {}", index + 1);
    };
    let lower = tag.to_ascii_lowercase().replace('_', "-");
    let head = lower.split('-').next().unwrap_or_default();
    name(&lower)
        .or_else(|| name(head))
        .map_or_else(|| tag.to_owned(), str::to_owned)
}

/// The blocks laid in place of `caption`: a link back, the tracks, the lines.
pub(crate) fn transcript(
    caption: &Block,
    card: &Card,
    (chosen, cues): (usize, &[Cue]),
    name: fn(&str) -> Option<&'static str>,
) -> Vec<Block> {
    let small = Emphasis {
        size: 90,
        ..Emphasis::default()
    };
    let role = match &caption.content {
        Content::Text(t) => t.role,
        _ => Role::Paragraph,
    };
    let mut head = Text::new(role);
    head.push_link("Hide transcript", small, card.toggle);
    let others = (card.tracks.iter().enumerate()).filter(|_| card.tracks.len() > 1);
    for ((i, track), pick) in others.zip(&card.picks) {
        head.push("  \u{b7}  ", small);
        let named = track_name(track, i, name);
        match i == chosen {
            true => head.push(
                &named,
                Emphasis {
                    bold: true,
                    ..small
                },
            ),
            false => head.push_link(&named, small, *pick),
        }
    }
    let mut out = vec![Block {
        content: Content::Text(head),
        frame: caption.frame.clone(),
        break_before: caption.break_before,
    }];
    let line = |text: Text| {
        let mut frame = caption.frame.clone();
        (frame.align, frame.tight) = (Align::Start, true);
        Block {
            content: Content::Text(text),
            frame,
            break_before: false,
        }
    };
    if cues.is_empty() {
        let mut text = Text::new(Role::Paragraph);
        text.push("This track has no lines to show.", small);
        out.push(line(text));
    }
    let stamp_look = Emphasis {
        size: 80,
        colour: Some(STAMP),
        ..Emphasis::default()
    };
    for cue in cues {
        let mut text = Text::new(Role::Paragraph);
        text.push(&stamp(cue.start), stamp_look);
        text.push("\u{2003}", stamp_look);
        text.push(&cue.text, small);
        out.push(line(text));
    }
    out
}

/// `m:ss`, or `h:mm:ss` past an hour.
pub fn stamp(seconds: f64) -> String {
    // Held to a million hours, so the cast keeps the value.
    let total = seconds.clamp(0.0, 3.6e9) as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    match h {
        0 => format!("{m}:{s:02}"),
        _ => format!("{h}:{m:02}:{s:02}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(language: Option<&str>, default: bool) -> Track {
        Track {
            path: "/t.vtt".to_owned(),
            language: language.map(str::to_owned),
            label: None,
            default,
        }
    }

    #[test]
    fn webvtt_cues_lose_their_tags_and_keep_their_speaker() {
        let vtt = "\u{feff}WEBVTT\n\nNOTE a note\n\nSTYLE\n::cue { color: red }\n\n\
                   one\n00:01.000 --> 00:04.500 align:start\n<v Ann>Hello &amp; <i>welcome</i></v>\n\
                   to the sea\n\n01:02:03.250 --> 01:02:05.000\n<c.loud>Waves</c> &#x263A;\n";
        let got = cues(vtt.as_bytes());
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].text, "Ann: Hello & welcome to the sea");
        assert_eq!((got[0].start, got[0].end), (1.0, 4.5));
        assert_eq!(got[1].text, "Waves \u{263a}");
        assert_eq!(got[1].start, 3723.25);
    }

    #[test]
    fn subrip_cues_read_with_comma_times() {
        let srt = "1\r\n00:00:02,500 --> 00:00:05,000\r\nFirst line\r\nsecond\r\n\r\n\
                   2\r\n00:00:06,000 --> 00:00:07,000\r\n<b>Bold</b>\r\n";
        let got = cues(srt.as_bytes());
        let texts: Vec<_> = got.iter().map(|c| (c.start, c.text.as_str())).collect();
        assert_eq!(texts, [(2.5, "First line second"), (6.0, "Bold")]);
    }

    #[test]
    fn ttml_paragraphs_read_with_their_times() {
        let xml = r#"<?xml version="1.0"?><tt xmlns="http://www.w3.org/ns/ttml"><body><div>
            <p begin="00:00:01.000" end="00:00:02.000">A <span>quiet</span><br/>sea</p>
            <p begin="3s" dur="1.5s">Tom &amp; Jerry</p>
            <p begin="00:00:10:15" end="00:00:11:00">Frames</p></div></body></tt>"#;
        let got = cues(xml.as_bytes());
        let texts: Vec<_> = got
            .iter()
            .map(|c| (c.start, c.end, c.text.as_str()))
            .collect();
        assert_eq!(
            texts,
            [
                (1.0, 2.0, "A quiet sea"),
                (3.0, 4.5, "Tom & Jerry"),
                (10.5, 11.0, "Frames")
            ]
        );
    }

    #[test]
    fn rolling_repeats_and_broken_blocks_are_dropped() {
        let vtt = "WEBVTT\n\n00:01.000 --> 00:02.000\nSame\n\n00:02.000 --> 00:03.000\nSame\n\n\
                   garbage --> nonsense\nLost\n\n00:05.000 --> 00:06.000\n\n";
        let got = cues(vtt.as_bytes());
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(cues(b"").is_empty() && cues(&[0xff, 0xfe, 0x00]).is_empty());
    }

    #[test]
    fn the_wanted_language_wins_then_the_default_then_the_first() {
        let tracks = [
            track(Some("en"), false),
            track(Some("fr-CA"), true),
            track(Some("my"), false),
        ];
        let want = |tags: &[&str]| -> Vec<String> { tags.iter().map(|t| t.to_string()).collect() };
        assert_eq!(pick(&tracks, &want(&["my"])), Some(2));
        assert_eq!(pick(&tracks, &want(&["fr"])), Some(1));
        assert_eq!(pick(&tracks, &want(&["en_GB", "my"])), Some(0));
        assert_eq!(pick(&tracks, &want(&["de"])), Some(1));
        assert_eq!(pick(&tracks[..1], &want(&["de"])), Some(0));
        assert_eq!(pick(&[], &want(&["de"])), None);
    }

    #[test]
    fn tracks_are_named_by_label_then_language() {
        let name = |t: &str| (t == "fr").then_some("French");
        let mut t = track(Some("fr-CA"), false);
        assert_eq!(track_name(&t, 0, name), "French");
        t.label = Some("Québécois".to_owned());
        assert_eq!(track_name(&t, 0, name), "Québécois");
        assert_eq!(track_name(&track(Some("xx"), false), 0, name), "xx");
        assert_eq!(track_name(&track(None, false), 2, name), "Track 3");
        assert_eq!(stamp(65.9), "1:05");
        assert_eq!(stamp(3725.0), "1:02:05");
    }

    #[test]
    fn formats_are_told_by_their_contents() {
        assert_eq!(Format::of(b"WEBVTT\n").extension(), "vtt");
        assert_eq!(Format::of(b"  <tt xmlns=''/>").extension(), "ttml");
        assert_eq!(
            Format::of(b"1\n00:00:01,000 --> 00:00:02,000").extension(),
            "srt"
        );
    }
}
