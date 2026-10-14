//! GitHub-flavoured Markdown as chapters: each `#` heading opens one.
use std::collections::HashMap;

use pulldown_cmark::{BlockQuoteKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::{Draft, Entry, MAX_PICTURE_FILE, Picture, Pictures, chapter_file, escape, fold_lines};
use crate::MAX_PICTURE_BYTES;

/// Headings this deep or shallower make the contents, as pandoc's default.
const TOC_DEPTH: u8 = 3;

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_GFM
        | Options::ENABLE_DEFINITION_LIST
}

/// What the first pass learns before any markup is written.
#[derive(Default)]
struct Scan {
    /// Event indices where a chapter after the first begins.
    starts: Vec<usize>,
    headings: Vec<Heading>,
    /// Every heading id, and the chapter holding it.
    ids: HashMap<String, usize>,
    /// Footnotes by label: number and the chapter defining it.
    notes: HashMap<String, Note>,
    /// Picture destinations and their paths in the book.
    pictures: HashMap<String, String>,
    matter: String,
}

struct Heading {
    level: u8,
    id: String,
    text: String,
    chapter: usize,
}

struct Note {
    number: usize,
    chapter: Option<usize>,
}

pub(super) fn draft(text: &str, title: Option<String>, pictures: Pictures<'_>) -> Draft {
    let text = fold_lines(text);
    let events: Vec<Event<'_>> = Parser::new_ext(&text, options()).collect();
    let mut d = Draft::new(None);
    let scan = scan(&events, &mut d, pictures);
    let matter = |key: &[&str]| front_matter(&scan.matter, key);
    let first_heading = scan.headings.iter().find(|h| h.level <= TOC_DEPTH);
    d.title = matter(&["title"])
        .or_else(|| first_heading.map(|h| h.text.clone()))
        .or(title)
        .unwrap_or(d.title);
    d.author = matter(&["author", "authors", "creator"]);
    d.language = matter(&["lang", "language"]);
    d.toc = scan
        .headings
        .iter()
        .filter(|h| h.level <= TOC_DEPTH && !h.text.is_empty())
        .map(|h| Entry {
            title: h.text.clone(),
            level: h.level,
            chapter: h.chapter,
            id: Some(h.id.clone()),
        })
        .collect();
    d.chapters = write(events, &scan);
    if d.toc.is_empty() {
        d.toc.push(Entry {
            title: d.title.clone(),
            level: 1,
            chapter: 0,
            id: None,
        });
    }
    d
}

fn level(l: HeadingLevel) -> u8 {
    l as u8
}

/// Chapters, ids, footnotes and pictures, in one walk.
fn scan(events: &[Event<'_>], d: &mut Draft, pictures: Pictures<'_>) -> Scan {
    let mut s = Scan::default();
    let mut chapter = 0;
    let mut started = false;
    let mut in_matter = false;
    let mut heading: Option<(u8, Option<String>, String)> = None;
    let mut budget = MAX_PICTURE_BYTES;
    for (i, e) in events.iter().enumerate() {
        match e {
            Event::Start(Tag::MetadataBlock(_)) => in_matter = true,
            Event::End(TagEnd::MetadataBlock(_)) => in_matter = false,
            Event::Text(t) if in_matter => s.matter.push_str(t),
            Event::Start(Tag::Heading { level: l, id, .. }) => {
                if *l == HeadingLevel::H1 && started {
                    chapter += 1;
                    s.starts.push(i);
                }
                let id = id.as_ref().map(|id| id.to_string());
                heading = Some((level(*l), id, String::new()));
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, _, text)) = &mut heading {
                    text.push_str(t);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, id, text)) = heading.take() {
                    let text = text.trim().to_owned();
                    let id = id.unwrap_or_else(|| unique(&slug(&text), &s.ids));
                    s.ids.insert(id.clone(), chapter);
                    s.headings.push(Heading {
                        level,
                        id,
                        text,
                        chapter,
                    });
                }
            }
            Event::FootnoteReference(label) => {
                let number = s.notes.len() + 1;
                s.notes.entry(label.to_string()).or_insert(Note {
                    number,
                    chapter: None,
                });
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                let number = s.notes.len() + 1;
                let note = s.notes.entry(label.to_string()).or_insert(Note {
                    number,
                    chapter: None,
                });
                note.chapter = Some(chapter);
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                let dest = dest_url.to_string();
                if !s.pictures.contains_key(&dest)
                    && let Some(href) = picture(&dest, d, pictures, &mut budget)
                {
                    s.pictures.insert(dest, href);
                }
            }
            _ => {}
        }
        let matter = matches!(
            e,
            Event::Start(Tag::MetadataBlock(_)) | Event::End(TagEnd::MetadataBlock(_))
        );
        started |= !in_matter && !matter;
    }
    s
}

/// Reads a local picture into the book; its path there, or none.
fn picture(dest: &str, d: &mut Draft, read: Pictures<'_>, budget: &mut usize) -> Option<String> {
    let path = local(dest)?;
    let bytes = read(&path).filter(|b| b.len() <= MAX_PICTURE_FILE && b.len() <= *budget)?;
    let (media, ext) = media(&bytes)?;
    *budget -= bytes.len();
    let href = format!("images/{}.{ext}", d.pictures.len() + 1);
    d.pictures.push(Picture {
        href: href.clone(),
        media,
        bytes,
    });
    Some(href)
}

/// A relative or absolute file path; none for the web, data or anchors.
fn local(dest: &str) -> Option<String> {
    let dest = dest.trim();
    let remote = dest.contains("://") || dest.starts_with("data:") || dest.starts_with("mailto:");
    if dest.is_empty() || remote || dest.starts_with('#') {
        return None;
    }
    let path = dest.split(['?', '#']).next().unwrap_or(dest);
    Some(percent_decode(path))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            Some(v) if b == b'%' => {
                out.push(v);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The kinds the reader decodes, told by their first bytes.
fn media(b: &[u8]) -> Option<(&'static str, &'static str)> {
    if b.starts_with(b"\x89PNG") {
        Some(("image/png", "png"))
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else if b.starts_with(b"GIF8") {
        Some(("image/gif", "gif"))
    } else if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

/// GitHub's heading slug, so `[x](#a-heading)` links land.
fn slug(text: &str) -> String {
    let mut s = String::new();
    for c in text.trim().chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            s.push(c);
        } else if c == ' ' {
            s.push('-');
        }
    }
    if s.is_empty() { "section".into() } else { s }
}

/// `base`, or `base-1`, `base-2` … when taken, as GitHub numbers them.
fn unique(base: &str, taken: &HashMap<String, usize>) -> String {
    if !taken.contains_key(base) {
        return base.to_owned();
    }
    (1..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken.contains_key(id))
        .unwrap_or_else(|| base.to_owned())
}

/// The events rewritten for the book, then each chapter's markup.
fn write<'a>(events: Vec<Event<'a>>, s: &Scan) -> Vec<String> {
    let mut chapters: Vec<Vec<Event<'a>>> = vec![Vec::new()];
    let mut chapter = 0;
    let mut headings = s.headings.iter();
    let mut starts = s.starts.iter().peekable();
    let mut in_matter = false;
    let mut pending_note = None;
    let mut ruled = false;
    let mut events = events.into_iter().enumerate().peekable();
    while let Some((i, e)) = events.next() {
        if starts.next_if(|&&at| at == i).is_some() {
            chapter += 1;
            chapters.push(Vec::new());
            ruled = false;
        }
        let mut out: Vec<Event<'a>> = Vec::new();
        match e {
            Event::Start(Tag::MetadataBlock(_)) => in_matter = true,
            Event::End(TagEnd::MetadataBlock(_)) => in_matter = false,
            _ if in_matter => {}
            Event::Start(Tag::Heading { level, .. }) => {
                let id = headings.next().map(|h| CowStr::from(h.id.clone()));
                out.push(Event::Start(Tag::Heading {
                    level,
                    id,
                    classes: Vec::new(),
                    attrs: Vec::new(),
                }));
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = match dest_url.strip_prefix('#').and_then(|f| s.ids.get(f)) {
                    Some(&at) if at != chapter => format!("{}{dest_url}", chapter_file(at)).into(),
                    _ => dest_url,
                };
                out.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = match s.pictures.get(dest_url.as_ref()) {
                    Some(href) => format!("../{href}").into(),
                    None => dest_url,
                };
                out.push(Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                }));
            }
            Event::FootnoteReference(label) => {
                out.push(Event::InlineHtml(reference(&label, chapter, s).into()));
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                let number = s.notes.get(label.as_ref()).map_or(0, |n| n.number);
                let rule = if ruled { "" } else { "<hr/>\n" };
                ruled = true;
                pending_note = Some(number);
                out.push(Event::Html(
                    format!("{rule}<div id=\"fn-{number}\">\n").into(),
                ));
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                pending_note = None;
                out.push(Event::Html("</div>\n".into()));
            }
            Event::Start(Tag::Paragraph) => {
                out.push(Event::Start(Tag::Paragraph));
                if let Some(number) = pending_note.take() {
                    out.push(Event::InlineHtml(format!("<sup>{number}</sup> ").into()));
                }
            }
            Event::Start(Tag::Item)
                if matches!(events.peek(), Some((_, Event::TaskListMarker(_)))) =>
            {
                // A task's box is its marker, as on GitHub.
                out.push(Event::Html("<li style=\"list-style: none\">".into()));
            }
            Event::TaskListMarker(done) => {
                // Squares every bundled face carries.
                out.push(Event::Text(
                    if done { "\u{25a0} " } else { "\u{25a1} " }.into(),
                ));
            }
            Event::Start(Tag::BlockQuote(Some(kind))) => {
                let label = alert(kind);
                let open = format!("<blockquote>\n<p><strong>{label}</strong></p>\n");
                out.push(Event::Html(open.into()));
            }
            Event::End(TagEnd::BlockQuote(Some(_))) => {
                out.push(Event::Html("</blockquote>\n".into()));
            }
            e => out.push(e),
        }
        if let Some(current) = chapters.last_mut() {
            current.append(&mut out);
        }
    }
    chapters
        .into_iter()
        .map(|events| {
            let mut html = String::new();
            pulldown_cmark::html::push_html(&mut html, events.into_iter());
            html
        })
        .collect()
}

/// A footnote mark linking to its note, wherever that chapter is.
fn reference(label: &str, chapter: usize, s: &Scan) -> String {
    let Some(note) = s.notes.get(label) else {
        return format!("<sup>[{}]</sup>", escape(label));
    };
    let n = note.number;
    match note.chapter {
        Some(at) if at == chapter => format!("<sup><a href=\"#fn-{n}\">{n}</a></sup>"),
        Some(at) => format!("<sup><a href=\"{}#fn-{n}\">{n}</a></sup>", chapter_file(at)),
        None => format!("<sup>{n}</sup>"),
    }
}

fn alert(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "Note",
        BlockQuoteKind::Tip => "Tip",
        BlockQuoteKind::Important => "Important",
        BlockQuoteKind::Warning => "Warning",
        BlockQuoteKind::Caution => "Caution",
    }
}

/// A top-level front matter value, YAML or TOML; a list gives its first.
fn front_matter(matter: &str, keys: &[&str]) -> Option<String> {
    let mut lines = matter.lines();
    while let Some(line) = lines.next() {
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((key, value)) = line.split_once([':', '=']) else {
            continue;
        };
        if !keys.contains(&key.trim().to_lowercase().as_str()) {
            continue;
        }
        let mut value = clean(value);
        if value.is_empty() {
            let item = lines.next().and_then(|l| l.trim_start().strip_prefix("- "));
            value = item.map(clean).unwrap_or_default();
        } else if let Some(list) = value.strip_prefix('[') {
            // `["Ann", "Bo"]`: the first name.
            let first = list.trim_end_matches(']').split(',').next().unwrap_or("");
            value = clean(first);
        }
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

fn clean(value: &str) -> String {
    let value = value.split(" #").next().unwrap_or(value).trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);
    unquoted.trim().to_owned()
}
