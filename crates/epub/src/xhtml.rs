//! Chapter XHTML to blocks: html5ever's tokenizer, our element stack, no DOM.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use mareader_reflow::{
    Align, Anchor, Block, Content, Emphasis, Frame, Picture, Role, Row, Table, Text,
};

use crate::css::{self, Decl, MAX_SHEET, Stylesheet};
use crate::href;

/// Open elements tracked at once; deeper nesting is read as flat.
pub const MAX_DEPTH: usize = 256;
const MAX_COLUMNS: usize = 32;
const MAX_BARS: usize = 8;
const MAX_CLASSES: usize = 16;
/// Stylesheets one chapter applies; later ones are ignored.
pub const MAX_SHEETS: usize = 16;
/// Indents in body ems, as mareader's stylesheet has them.
const LIST_INSET: f32 = 1.6;
const QUOTE_INSET: f32 = 1.0;
const TAB: &str = "    ";
/// Ids and links kept per book; past these, the rest are ignored.
pub const MAX_PLACES: usize = 1 << 20;
pub const MAX_LINKS: usize = 1 << 20;

/// Where a link leads, as its chapter wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Href {
    /// `path` or `path#id` in the archive, both decoded.
    Inside(String),
    /// Anything with a scheme, checked only when followed.
    Outside(String),
}

/// The places a book's ids mark and its links' destinations.
#[derive(Debug, Default)]
pub(crate) struct Refs {
    /// `path` and `path#id` to where each starts.
    pub(crate) places: HashMap<String, Anchor>,
    /// Indexed by the target a link's text carries.
    pub(crate) hrefs: Vec<Href>,
}

impl Refs {
    /// The place `key` marks, else the start of its file.
    pub(crate) fn place(&self, key: &str) -> Option<Anchor> {
        let file = key.split('#').next().unwrap_or_default();
        self.places
            .get(key)
            .or_else(|| self.places.get(file))
            .copied()
    }
}

/// Where a chapter's images and stylesheets come from.
pub(crate) trait Library {
    /// `path` is absolute in the archive; `None` if missing or unusable.
    fn picture(&mut self, path: &str) -> Option<Arc<Picture>>;
    fn stylesheet(&mut self, path: &str) -> Option<Arc<Stylesheet>>;
}

/// What open elements pass down to the text inside them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Style {
    italic: bool,
    bold: bool,
    code: bool,
    pre: bool,
    align: Align,
    /// Not shown at all: the head, scripts, `display: none`.
    hidden: bool,
    /// Inside SVG or MathML: no text, though SVG images still show.
    graphic: bool,
    /// Inside a link, by its index in [`Refs::hrefs`].
    link: Option<u32>,
}

impl Style {
    fn apply(&mut self, d: &Decl) {
        self.italic = d.italic.unwrap_or(self.italic);
        self.bold = d.bold.unwrap_or(self.bold);
        self.align = d.align.unwrap_or(self.align);
        self.hidden |= d.hidden.unwrap_or(false);
    }

    fn emphasis(&self) -> Emphasis {
        Emphasis {
            italic: self.italic,
            bold: self.bold,
            code: self.code,
        }
    }
}

struct Open {
    name: String,
    style: Style,
    /// An ordered list's last item number.
    counter: u32,
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Elements that end the text before them and start anew.
fn is_block(name: &str) -> bool {
    heading(name).is_some()
        || matches!(
            name,
            "p" | "div"
                | "li"
                | "ul"
                | "ol"
                | "dl"
                | "dt"
                | "dd"
                | "blockquote"
                | "pre"
                | "section"
                | "article"
                | "header"
                | "footer"
                | "aside"
                | "nav"
                | "figure"
                | "figcaption"
                | "body"
                | "main"
                | "address"
                | "center"
                | "table"
                | "caption"
                | "thead"
                | "tbody"
                | "tfoot"
                | "tr"
                | "td"
                | "th"
                | "hr"
        )
}

fn heading(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Elements whose content the tokenizer must read as raw text.
fn is_raw(name: &str) -> bool {
    matches!(
        name,
        "style" | "script" | "title" | "noscript" | "template" | "textarea" | "xmp"
    )
}

/// HTML's collapsible spaces; U+00A0 and friends stay as written.
fn is_html_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn attr<'t>(tag: &'t Tag, name: &str) -> Option<&'t str> {
    tag.attrs
        .iter()
        .find(|a| &*a.name.local == name)
        .map(|a| &*a.value)
}

/// The table being read; only the outermost one builds rows.
struct TableBuild {
    table: Table,
    frame: Frame,
    caption: Text,
    /// Tables inside this one, read as text in its cells.
    nested: usize,
    in_cell: bool,
    in_caption: bool,
    in_head: bool,
}

impl TableBuild {
    fn cell(&mut self) -> Option<&mut Text> {
        if self.in_caption {
            return Some(&mut self.caption);
        }
        if !self.in_cell {
            return None;
        }
        self.table.rows.last_mut()?.cells.last_mut()
    }
}

struct State<'a> {
    library: &'a mut dyn Library,
    refs: &'a mut Refs,
    /// The book's index of this chapter's first block.
    first: usize,
    /// Ids waiting for the next block, which they mark.
    pending: Vec<String>,
    /// The chapter's own path, for resolving its links.
    base: &'a str,
    sheets: Vec<Arc<Stylesheet>>,
    /// An inline `<style>` being read.
    style_text: String,
    blocks: Vec<Block>,
    current: Option<Block>,
    table: Option<TableBuild>,
    stack: Vec<Open>,
    /// A collapsed space waits for the next visible character.
    space: bool,
    /// The next block opens a chapter: it starts a page.
    chapter: bool,
    /// A list item's marker, waiting for its first block.
    marker: Option<String>,
    /// The first block of an outermost list keeps the full gap above.
    list_opened: bool,
    /// Just inside `<pre>`, where one leading newline is dropped.
    pre_opened: bool,
}

impl State<'_> {
    fn top(&self) -> Style {
        self.stack.last().map(|o| o.style).unwrap_or_default()
    }

    fn in_list(&self) -> bool {
        self.stack.iter().any(|o| o.name == "ul" || o.name == "ol")
    }

    /// Indents and quote bars from the open lists and quotes.
    fn frame(&mut self) -> Frame {
        let mut f = Frame::default();
        for open in &self.stack {
            match open.name.as_str() {
                "ul" | "ol" | "dd" => f.inset += LIST_INSET,
                "blockquote" => {
                    if f.bars.len() < MAX_BARS {
                        f.bars.push(f.inset);
                    }
                    f.inset += QUOTE_INSET;
                    f.soft = true;
                }
                _ => {}
            }
        }
        f.align = self.top().align;
        f.tight = self.in_list() && !std::mem::take(&mut self.list_opened);
        f
    }

    fn role(&self) -> Role {
        let level = self.stack.iter().rev().find_map(|o| heading(&o.name));
        match level {
            Some(level) => Role::Heading(level),
            None if self.top().pre => Role::Preformatted,
            None => Role::Paragraph,
        }
    }

    fn emit(&mut self, mut block: Block) {
        block.break_before = std::mem::take(&mut self.chapter);
        let at = Anchor {
            block: self.first + self.blocks.len(),
            offset: 0,
        };
        for key in self.pending.drain(..) {
            self.refs.places.entry(key).or_insert(at);
        }
        self.blocks.push(block);
    }

    /// Notes where `id` is: mid-paragraph, or the next block.
    fn mark(&mut self, id: &str) {
        let id = id.trim();
        if id.is_empty() || self.refs.places.len() + self.pending.len() >= MAX_PLACES {
            return;
        }
        let key = format!("{}#{id}", self.base);
        let written = match self.current.as_ref().map(|b| &b.content) {
            Some(Content::Text(t)) if self.table.is_none() => t.text.len(),
            _ => 0,
        };
        if written == 0 {
            return self.pending.push(key);
        }
        let at = Anchor {
            block: self.first + self.blocks.len(),
            offset: written,
        };
        self.refs.places.entry(key).or_insert(at);
    }

    /// `href`'s index in [`Refs::hrefs`], resolved from this chapter.
    fn link_to(&mut self, href: &str) -> Option<u32> {
        let href = href.trim();
        let fragment = href.split_once('#').map(|(_, f)| href::percent_decode(f));
        let fragment = fragment.filter(|f| !f.is_empty());
        let target = if let (true, Some(f)) = (href.starts_with('#'), &fragment) {
            Href::Inside(format!("{}#{f}", self.base))
        } else if let Some(path) = href::resolve(self.base, href) {
            match fragment {
                Some(f) => Href::Inside(format!("{path}#{f}")),
                None => Href::Inside(path),
            }
        } else if href.contains(':') {
            Href::Outside(href.to_owned())
        } else {
            return None;
        };
        if self.refs.hrefs.len() >= MAX_LINKS {
            return None;
        }
        let index = u32::try_from(self.refs.hrefs.len()).ok()?;
        self.refs.hrefs.push(target);
        Some(index)
    }

    fn finish(&mut self) {
        if let Some(mut block) = self.current.take() {
            if let Content::Text(text) = &mut block.content {
                text.trim_end();
            }
            if !block.is_blank() {
                self.emit(block);
            }
        }
        self.space = false;
    }

    /// The text that characters go to now, if any.
    fn target(&mut self) -> Option<&mut Text> {
        if self.table.is_some() {
            return self.table.as_mut().and_then(TableBuild::cell);
        }
        if self.current.is_none() {
            let mut frame = self.frame();
            frame.marker = self.marker.take();
            let mut block = Block::new(Content::Text(Text::new(self.role())));
            block.frame = frame;
            self.current = Some(block);
        }
        match self.current.as_mut().map(|b| &mut b.content) {
            Some(Content::Text(text)) => Some(text),
            _ => None,
        }
    }

    fn characters(&mut self, input: &str) {
        let style = self.top();
        if self.stack.last().is_some_and(|o| o.name == "style") {
            if self.style_text.len() + input.len() <= MAX_SHEET {
                self.style_text.push_str(input);
            }
            return;
        }
        if style.hidden || style.graphic {
            return;
        }
        let emphasis = style.emphasis();
        if style.pre {
            let mut text = input.replace('\r', "").replace('\t', TAB);
            if std::mem::take(&mut self.pre_opened) && text.starts_with('\n') {
                text.remove(0);
            }
            if let Some(t) = self.target() {
                put(t, &text, emphasis, style.link);
            }
            return;
        }
        if input.chars().all(is_html_space) {
            self.space |= !input.is_empty();
            return;
        }
        let mut space = self.space;
        let Some(target) = self.target() else {
            return;
        };
        let mut text = String::with_capacity(input.len());
        for c in input.chars() {
            if is_html_space(c) {
                space = true;
                continue;
            }
            let line_start =
                text.is_empty() && (target.text.is_empty() || target.text.ends_with('\n'));
            if std::mem::take(&mut space) && !line_start {
                text.push(' ');
            }
            text.push(c);
        }
        put(target, &text, emphasis, style.link);
        self.space = space;
    }

    fn line_break(&mut self) {
        let pre = self.top().pre;
        let in_table = self.table.is_some();
        let target = if pre || in_table {
            self.target()
        } else {
            self.current.as_mut().and_then(|b| match &mut b.content {
                Content::Text(t) => Some(t),
                _ => None,
            })
        };
        if let Some(t) = target.filter(|t| pre || !t.text.is_empty()) {
            t.push("\n", Emphasis::default());
        }
        self.space = false;
    }

    /// A block boundary: new block outside tables, new line in a cell.
    fn boundary(&mut self, name: &str) {
        let Some(table) = self.table.as_mut() else {
            return self.finish();
        };
        let structural = matches!(
            name,
            "table" | "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" | "caption"
        );
        if !structural
            && let Some(cell) = table.cell()
            && !cell.text.is_empty()
            && !cell.text.ends_with('\n')
        {
            cell.push("\n", Emphasis::default());
        }
        self.space = false;
    }

    /// Ends elements HTML closes implicitly, like `<p>` before `<div>`.
    fn close_implied(&mut self, name: &str) {
        let closes: &[&str] = match name {
            "li" => &["p", "li"],
            "dt" | "dd" => &["p", "dt", "dd"],
            "tr" => &["p", "tr", "td", "th"],
            "td" | "th" => &["p", "td", "th"],
            _ if is_block(name) => &["p"],
            _ => &[],
        };
        while let Some(at) = self.stack.iter().rposition(|o| is_block(&o.name)) {
            let innermost = self.stack.get(at).map(|o| o.name.as_str());
            if !innermost.is_some_and(|n| closes.contains(&n)) {
                break;
            }
            self.stack.truncate(at);
        }
    }

    fn next_marker(&mut self) -> String {
        let depth = self
            .stack
            .iter()
            .filter(|o| o.name == "ul" || o.name == "ol")
            .count();
        let list = self
            .stack
            .iter_mut()
            .rev()
            .find(|o| o.name == "ul" || o.name == "ol");
        match list {
            Some(ol) if ol.name == "ol" => {
                ol.counter = ol.counter.saturating_add(1);
                format!("{}.", ol.counter)
            }
            _ => match depth % 3 {
                2 => "◦",
                0 if depth > 0 => "▪",
                _ => "•",
            }
            .to_owned(),
        }
    }

    fn style_for(&self, tag: &Tag, name: &str) -> Style {
        let mut st = self.top();
        match name {
            "em" | "i" | "cite" | "var" | "dfn" | "address" => st.italic = true,
            "strong" | "b" => st.bold = true,
            "code" | "kbd" | "samp" | "tt" => st.code = true,
            "pre" | "listing" | "xmp" => st.pre = true,
            "center" => st.align = Align::Center,
            "svg" | "math" => st.graphic = true,
            "head" | "style" | "script" | "title" | "template" | "noscript" => st.hidden = true,
            _ => {}
        }
        if let Some(a) = attr(tag, "align").and_then(css::align) {
            st.align = a;
        }
        let classes: Vec<&str> = attr(tag, "class")
            .map(|c| c.split_ascii_whitespace().take(MAX_CLASSES).collect())
            .unwrap_or_default();
        let mut found = Vec::new();
        for (sheet, s) in self.sheets.iter().enumerate() {
            s.matching(name, &classes, &mut |spec, at, d| {
                found.push((spec, sheet, at, d))
            });
        }
        found.sort_by_key(|&(spec, sheet, at, _)| (spec, sheet, at));
        for (_, _, _, d) in &found {
            st.apply(d);
        }
        if let Some(inline) = attr(tag, "style") {
            st.apply(&Decl::parse(inline));
        }
        st
    }

    /// Every element meets every sheet, so a chapter keeps a few, once.
    fn add_sheet(&mut self, sheet: Arc<Stylesheet>) {
        let known = self.sheets.iter().any(|s| Arc::ptr_eq(s, &sheet));
        if !known && self.sheets.len() < MAX_SHEETS {
            self.sheets.push(sheet);
        }
    }

    fn link(&mut self, tag: &Tag) {
        let sheet = attr(tag, "rel").is_some_and(|r| {
            r.split_ascii_whitespace()
                .any(|w| w.eq_ignore_ascii_case("stylesheet"))
        });
        let path = attr(tag, "href").and_then(|h| href::resolve(self.base, h));
        if let (true, Some(path)) = (sheet, path)
            && let Some(sheet) = self.library.stylesheet(&path)
        {
            self.add_sheet(sheet);
        }
    }

    /// Each image is its own block; one that won't load shows its alt.
    fn image(&mut self, src: Option<&str>, alt: Option<&str>) {
        if self.top().hidden {
            return;
        }
        let alt = alt.map(str::trim).filter(|a| !a.is_empty());
        if self.table.is_some() {
            if let (Some(alt), Some(cell)) = (alt, self.target()) {
                cell.push(alt, Emphasis::default());
            }
            return;
        }
        let path = src.and_then(|s| href::resolve(self.base, s));
        let picture = path.and_then(|p| self.library.picture(&p));
        self.finish();
        if let Some(picture) = picture {
            let mut block = Block::new(Content::Image {
                picture,
                cover: false,
            });
            block.frame = self.frame();
            self.emit(block);
        } else if let Some(alt) = alt {
            let italic = Emphasis {
                italic: true,
                ..Emphasis::default()
            };
            if let Some(t) = self.target() {
                t.push(alt, italic);
            }
            self.finish();
        }
    }

    fn start_table(&mut self, tag_name: &str, tag: &Tag) {
        if tag_name == "table" {
            if let Some(t) = self.table.as_mut() {
                t.nested += 1;
            } else {
                let frame = self.frame();
                self.table = Some(TableBuild {
                    table: Table::default(),
                    frame,
                    caption: Text::new(Role::Paragraph),
                    nested: 0,
                    in_cell: false,
                    in_caption: false,
                    in_head: false,
                });
            }
            return;
        }
        let Some(t) = self.table.as_mut() else {
            return;
        };
        if t.nested > 0 {
            if matches!(tag_name, "td" | "th")
                && let Some(cell) = t.cell()
            {
                cell.push(" ", Emphasis::default());
            }
            return;
        }
        match tag_name {
            "thead" => t.in_head = true,
            "tbody" | "tfoot" => t.in_head = false,
            "caption" => t.in_caption = !tag.self_closing,
            "tr" => t.table.rows.push(Row {
                cells: Vec::new(),
                head: t.in_head,
            }),
            "td" | "th" => {
                if t.table.rows.is_empty() {
                    t.table.rows.push(Row {
                        cells: Vec::new(),
                        head: t.in_head,
                    });
                }
                let in_head = t.in_head;
                if let Some(row) = t.table.rows.last_mut() {
                    let head = in_head || tag_name == "th";
                    row.head = if row.cells.is_empty() {
                        head
                    } else {
                        row.head && head
                    };
                    if row.cells.len() < MAX_COLUMNS {
                        row.cells.push(Text::new(Role::Paragraph));
                    }
                }
                t.in_cell = true;
            }
            _ => {}
        }
    }

    fn end_table(&mut self, name: &str) {
        let Some(t) = self.table.as_mut() else {
            return;
        };
        match name {
            "table" if t.nested > 0 => t.nested -= 1,
            "table" => self.flush_table(),
            "td" | "th" if t.nested == 0 => t.in_cell = false,
            "caption" => t.in_caption = false,
            "thead" => t.in_head = false,
            _ => {}
        }
    }

    /// The caption, centred, then the table; empty ones vanish.
    fn flush_table(&mut self) {
        let Some(t) = self.table.take() else {
            return;
        };
        let mut caption = t.caption;
        caption.trim_end();
        if !caption.is_blank() {
            let mut block = Block::new(Content::Text(caption));
            block.frame = Frame {
                align: Align::Center,
                ..t.frame.clone()
            };
            self.emit(block);
        }
        let mut table = t.table;
        for cell in table.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
            cell.trim_end();
        }
        table.rows.retain(|r| !r.cells.is_empty());
        let mut block = Block::new(Content::Table(table));
        block.frame = t.frame;
        if !block.is_blank() {
            self.emit(block);
        }
    }

    /// Returns whether the tokenizer should read raw text next.
    fn start(&mut self, tag: &Tag) -> bool {
        let name: &str = &tag.name;
        self.close_implied(name);
        let id = attr(tag, "id").or_else(|| attr(tag, "name").filter(|_| name == "a"));
        if let Some(id) = id.filter(|_| !is_block(name)) {
            self.mark(id);
        }
        match name {
            "br" => {
                self.line_break();
                return false;
            }
            "hr" => {
                self.boundary(name);
                if self.table.is_none() && !self.top().hidden {
                    let mut rule = Block::new(Content::Rule);
                    rule.frame = self.frame();
                    self.emit(rule);
                }
                return false;
            }
            "img" => {
                self.image(attr(tag, "src"), attr(tag, "alt"));
                return false;
            }
            "image" => {
                let href = attr(tag, "href").or_else(|| attr(tag, "xlink:href"));
                self.image(href, None);
            }
            "link" => {
                self.link(tag);
                return false;
            }
            _ => {}
        }
        if is_block(name) {
            self.boundary(name);
            if let Some(id) = id {
                self.mark(id);
            }
        }
        if matches!(
            name,
            "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "caption"
        ) {
            self.start_table(name, tag);
        }
        if name == "li" {
            self.marker = Some(self.next_marker());
        }
        if is_void(name) || tag.self_closing || self.stack.len() >= MAX_DEPTH {
            return false;
        }
        if matches!(name, "ul" | "ol") && !self.in_list() {
            self.list_opened = true;
        }
        let counter = attr(tag, "start")
            .and_then(|s| s.trim().parse::<u32>().ok())
            .map_or(0, |s| s.saturating_sub(1));
        let mut style = self.style_for(tag, name);
        if let Some(href) = attr(tag, "href").filter(|_| name == "a") {
            style.link = self.link_to(href);
        }
        self.pre_opened = name == "pre";
        self.stack.push(Open {
            name: name.to_owned(),
            style,
            counter,
        });
        if name == "style" {
            self.style_text.clear();
        }
        is_raw(name)
    }

    fn end(&mut self, name: &str) {
        if name == "style" && self.stack.last().is_some_and(|o| o.name == "style") {
            let sheet = Stylesheet::parse(&std::mem::take(&mut self.style_text));
            self.add_sheet(Arc::new(sheet));
        }
        if is_block(name) {
            self.boundary(name);
        }
        self.end_table(name);
        // Close the innermost match and anything left open in it.
        if let Some(at) = self.stack.iter().rposition(|o| o.name == name) {
            self.stack.truncate(at);
        }
        if matches!(name, "ul" | "ol") && !self.in_list() {
            self.list_opened = false;
        }
    }

    fn eof(&mut self) {
        self.finish();
        self.flush_table();
        // Ids after the last block mark where the next chapter starts.
        let at = Anchor {
            block: self.first + self.blocks.len(),
            offset: 0,
        };
        for key in self.pending.drain(..) {
            self.refs.places.entry(key).or_insert(at);
        }
    }
}

fn put(text: &mut Text, s: &str, emphasis: Emphasis, link: Option<u32>) {
    match link {
        Some(target) => text.push_link(s, emphasis, target),
        None => text.push(s, emphasis),
    }
}

struct Sink<'a>(RefCell<State<'a>>);

impl TokenSink for Sink<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut s = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) => match tag.kind {
                TagKind::StartTag => {
                    if s.start(&tag) {
                        return TokenSinkResult::RawData(RawKind::Rawtext);
                    }
                }
                TagKind::EndTag => s.end(&tag.name),
            },
            Token::CharacterTokens(text) => s.characters(&text),
            Token::EOFToken => s.eof(),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

/// Appends the blocks of `xhtml`, found at `path`; the first starts a page.
pub(crate) fn read_chapter(
    xhtml: &str,
    path: &str,
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
) {
    let first = out.len();
    let start = Anchor {
        block: first,
        offset: 0,
    };
    refs.places.entry(path.to_owned()).or_insert(start);
    let sink = Sink(RefCell::new(State {
        library,
        refs,
        first,
        pending: Vec::new(),
        base: path,
        sheets: Vec::new(),
        style_text: String::new(),
        blocks: Vec::new(),
        current: None,
        table: None,
        stack: Vec::new(),
        space: false,
        chapter: true,
        marker: None,
        list_opened: false,
        pre_opened: false,
    }));
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(xhtml));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    out.extend(state.blocks);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// In-memory images and stylesheets, keyed by absolute path.
    #[derive(Default)]
    struct Shelf {
        pictures: HashMap<String, Arc<Picture>>,
        sheets: HashMap<String, Arc<Stylesheet>>,
    }

    impl Library for Shelf {
        fn picture(&mut self, path: &str) -> Option<Arc<Picture>> {
            self.pictures.get(path).cloned()
        }

        fn stylesheet(&mut self, path: &str) -> Option<Arc<Stylesheet>> {
            self.sheets.get(path).cloned()
        }
    }

    const BASE: &str = "/OEBPS/text/c1.xhtml";

    fn read_refs(xhtml: &str) -> (Vec<Block>, Refs) {
        let (mut out, mut refs) = (Vec::new(), Refs::default());
        read_chapter(xhtml, BASE, &mut Shelf::default(), (&mut out, &mut refs));
        (out, refs)
    }

    fn read_with(shelf: &mut Shelf, xhtml: &str) -> Vec<Block> {
        let mut out = Vec::new();
        read_chapter(xhtml, BASE, shelf, (&mut out, &mut Refs::default()));
        out
    }

    fn read(xhtml: &str) -> Vec<Block> {
        read_with(&mut Shelf::default(), xhtml)
    }

    fn text(b: &Block) -> Option<&Text> {
        match &b.content {
            Content::Text(t) => Some(t),
            _ => None,
        }
    }

    fn texts(blocks: &[Block]) -> Vec<&str> {
        blocks
            .iter()
            .filter_map(text)
            .map(|t| t.text.as_str())
            .collect()
    }

    fn role(b: Option<&Block>) -> Option<Role> {
        b.and_then(text).map(|t| t.role)
    }

    fn spans(b: Option<&Block>) -> Vec<(&str, Emphasis)> {
        let Some(t) = b.and_then(text) else {
            return Vec::new();
        };
        t.spans
            .iter()
            .filter_map(|s| Some((t.text.get(s.range.clone())?, s.emphasis)))
            .collect()
    }

    fn png(w: u32, h: u32) -> Option<Arc<Picture>> {
        let img = image::RgbImage::new(w, h);
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Picture::probe(out.into_inner()).ok().map(Arc::new)
    }

    #[test]
    fn headings_and_paragraphs_become_blocks() {
        let b = read(
            "<?xml version='1.0'?><html><head><title>T</title>\
             <style>p { color: red }</style></head><body>\
             <h2>Chapter  One</h2><p>First\n   para.</p><p>Second.</p></body></html>",
        );
        assert_eq!(texts(&b), ["Chapter One", "First para.", "Second."]);
        assert_eq!(role(b.first()), Some(Role::Heading(2)));
        assert_eq!(role(b.get(1)), Some(Role::Paragraph));
    }

    #[test]
    fn only_the_first_block_starts_a_page() {
        let b = read("<p>a</p><p>b</p>");
        let breaks: Vec<bool> = b.iter().map(|b| b.break_before).collect();
        assert_eq!(breaks, [true, false]);
    }

    #[test]
    fn emphasis_marks_spans() {
        let b = read("<p>a <em>b <strong>c</strong></em> <code>d</code></p>");
        assert_eq!(texts(&b), ["a b c d"]);
        let e = |italic, bold, code| Emphasis { italic, bold, code };
        // A collapsed space joins the run after it.
        assert_eq!(
            spans(b.first()),
            [
                (" b", e(true, false, false)),
                (" c", e(true, true, false)),
                (" d", e(false, false, true))
            ]
        );
    }

    #[test]
    fn self_closing_tags_open_nothing() {
        let b = read(
            "<head><script src='x.js'/><link rel='stylesheet'/></head>\
             <p><a id='n1'/>Kept <em/>plain.</p>",
        );
        assert_eq!(texts(&b), ["Kept plain."]);
        assert!(spans(b.first()).is_empty());
    }

    #[test]
    fn unclosed_and_stray_tags_are_tolerated() {
        let b = read("<p>one<p>two</em></b><div>three</span>");
        assert_eq!(texts(&b), ["one", "two", "three"]);
    }

    #[test]
    fn no_break_spaces_are_kept() {
        let b = read("<p>a&nbsp;&nbsp;b   c</p>");
        assert_eq!(texts(&b), ["a\u{a0}\u{a0}b c"]);
    }

    #[test]
    fn line_breaks_survive_collapsing() {
        let b = read("<p>line one<br/>\n  line two</p>");
        assert_eq!(texts(&b), ["line one\nline two"]);
    }

    #[test]
    fn deep_nesting_is_read_flat() {
        let deep = "<em>".repeat(10_000) + "x" + &"</em>".repeat(10_000);
        let b = read(&format!("<p>{deep}</p>"));
        assert_eq!(texts(&b), ["x"]);
    }

    #[test]
    fn hidden_text_stays_hidden() {
        let b = read("<p>a<svg><text>no</text></svg>b<script>if (a<b) {}</script></p>");
        assert_eq!(texts(&b), ["ab"]);
    }

    #[test]
    fn lists_indent_mark_and_tighten_their_items() {
        let b = read(
            "<p>Before</p><ul><li>one</li><li>two<ul><li>deep</li></ul></li></ul>\
             <ol start='4'><li>four<li>five</ol><p>After</p>",
        );
        assert_eq!(
            texts(&b),
            ["Before", "one", "two", "deep", "four", "five", "After"]
        );
        let frames: Vec<(f32, Option<&str>, bool)> = b
            .iter()
            .map(|b| (b.frame.inset, b.frame.marker.as_deref(), b.frame.tight))
            .collect();
        assert_eq!(
            frames,
            [
                (0.0, None, false),
                (1.6, Some("•"), false),
                (1.6, Some("•"), true),
                (3.2, Some("◦"), true),
                (1.6, Some("4."), false),
                (1.6, Some("5."), true),
                (0.0, None, false),
            ]
        );
    }

    #[test]
    fn quotes_indent_with_a_bar_and_soften() {
        let b = read("<blockquote><p>said</p><blockquote>again</blockquote></blockquote>");
        let frames: Vec<(f32, &[f32], bool)> = b
            .iter()
            .map(|b| (b.frame.inset, b.frame.bars.as_slice(), b.frame.soft))
            .collect();
        assert_eq!(
            frames,
            [(1.0, &[0.0][..], true), (2.0, &[0.0, 1.0][..], true)]
        );
    }

    #[test]
    fn preformatted_text_keeps_its_spaces_and_lines() {
        let b = read("<pre>\nfn main() {\n\tgo(<b>1</b>);\n}\n</pre><p>a  b</p>");
        assert_eq!(texts(&b), ["fn main() {\n    go(1);\n}", "a b"]);
        assert_eq!(role(b.first()), Some(Role::Preformatted));
    }

    #[test]
    fn tables_read_rows_cells_and_heads() {
        let b = read(
            "<table><caption>Prices</caption><thead><tr><th>Item<th>Cost</thead>\
             <tbody><tr><td>Tea<td><p>3</p><p>each</p></tr>\
             <tr><td>Rice<table><tr><td>in</td><td>sacks</td></tr></table></td></tr>\
             </tbody></table><p>after</p>",
        );
        assert_eq!(texts(&b), ["Prices", "after"]);
        assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
        let table = b.get(1).and_then(|b| match &b.content {
            Content::Table(t) => Some(t),
            _ => None,
        });
        let rows: Vec<(Vec<&str>, bool)> = table
            .map(|t| t.rows.iter())
            .into_iter()
            .flatten()
            .map(|r| (r.cells.iter().map(|c| c.text.as_str()).collect(), r.head))
            .collect();
        assert_eq!(
            rows,
            [
                (vec!["Item", "Cost"], true),
                (vec!["Tea", "3\neach"], false),
                (vec!["Rice in sacks"], false),
            ]
        );
    }

    #[test]
    fn rules_and_images_are_blocks_and_missing_images_show_alt() {
        let mut shelf = Shelf::default();
        if let Some(p) = png(4, 2) {
            shelf.pictures.insert("/OEBPS/images/a.png".into(), p);
        }
        let b = read_with(
            &mut shelf,
            "<p>one</p><hr/><div class='c' style='text-align:center'>\
             <img src='../images/a.png' alt='A'/></div>\
             <img src='gone.png' alt='Gone'/><img src='https://x/y.png'/>\
             <svg><image xlink:href='../images/a.png'/></svg>",
        );
        let kinds: Vec<&str> = b
            .iter()
            .map(|b| match &b.content {
                Content::Text(_) => "text",
                Content::Rule => "rule",
                Content::Image { .. } => "image",
                Content::Table(_) => "table",
            })
            .collect();
        assert_eq!(kinds, ["text", "rule", "image", "text", "image"]);
        assert_eq!(b.get(2).map(|b| b.frame.align), Some(Align::Center));
        let italic = Emphasis {
            italic: true,
            ..Emphasis::default()
        };
        assert_eq!(spans(b.get(3)), [("Gone", italic)]);
    }

    #[test]
    fn stylesheets_style_by_tag_and_class() {
        let mut shelf = Shelf::default();
        let linked = Stylesheet::parse(".it { font-style: italic } .gone { display: none }");
        shelf
            .sheets
            .insert("/OEBPS/css/book.css".into(), Arc::new(linked));
        let b = read_with(
            &mut shelf,
            "<head><link rel='stylesheet' href='../css/book.css'/>\
             <style>p.c { text-align: center } span.b { font-weight: bold }</style></head>\
             <p class='c'>mid <span class='b it'>x</span></p>\
             <p class='gone'>hidden</p><p style='font-style:italic'>y</p>",
        );
        assert_eq!(texts(&b), ["mid x", "y"]);
        assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
        let e = |italic, bold| Emphasis {
            italic,
            bold,
            code: false,
        };
        assert_eq!(spans(b.first()), [(" x", e(true, true))]);
        assert_eq!(spans(b.get(1)), [("y", e(true, false))]);
    }

    #[test]
    fn a_chapter_applies_a_few_sheets_each_once() {
        let mut shelf = Shelf::default();
        let linked = Stylesheet::parse(".it { font-style: italic }");
        shelf
            .sheets
            .insert("/OEBPS/css/book.css".into(), Arc::new(linked));
        let links = "<link rel='stylesheet' href='../css/book.css'/>".repeat(100);
        let styles: String = (0..MAX_SHEETS)
            .map(|i| format!("<style>.c{i} {{ text-align: center }}</style>"))
            .collect();
        let last = MAX_SHEETS - 2;
        let body = format!(
            "<p class='c0'>a</p><p class='c{last}'>b</p><p class='c{}'>c</p>",
            last + 1
        );
        let b = read_with(&mut shelf, &format!("<head>{links}{styles}</head>{body}"));
        let aligns: Vec<Align> = b.iter().map(|b| b.frame.align).collect();
        // The linked sheet counts once, so the last style is one too many.
        assert_eq!(aligns, [Align::Center, Align::Center, Align::Start]);
    }

    #[test]
    fn ids_mark_their_block_or_their_place_in_its_text() {
        let (_, refs) = read_refs(
            "<h1 id='top'>Title</h1><p>ab <span id='mid'>c</span></p>\
             <div id='empty'></div><p><a name='old'></a>d</p>\
             <table><tr><td id='cell'>e</td></tr></table><p id='end'></p>",
        );
        let at = |key: &str| {
            let a = refs.place(&format!("{BASE}{key}"));
            a.map(|a| (a.block, a.offset))
        };
        assert_eq!(at(""), Some((0, 0)));
        assert_eq!(at("#top"), Some((0, 0)));
        assert_eq!(at("#mid"), Some((1, 2)));
        assert_eq!(at("#empty"), Some((2, 0)));
        assert_eq!(at("#old"), Some((2, 0)));
        assert_eq!(at("#cell"), Some((3, 0)));
        // Past the last block: where the next chapter will start.
        assert_eq!(at("#end"), Some((4, 0)));
        // An unknown id falls back to its file's start.
        assert_eq!(at("#nowhere"), Some((0, 0)));
    }

    #[test]
    fn links_mark_their_words_and_keep_where_they_lead() {
        let (blocks, refs) = read_refs(
            "<p>See <a href='c2.xhtml#n%201'>the <i>first</i> note</a> \
             or <a href='https://example.org/'>a site</a>.</p>\
             <p><a href='#top'>Back</a> <a href='javascript:x()'>run</a>\
             <a href=''>none</a></p>",
        );
        let links = |b: Option<&Block>| {
            let t = b.and_then(text);
            let words = |l: &mareader_reflow::Link| {
                let s = t.and_then(|t| t.text.get(l.range.clone()));
                (s.unwrap_or_default().to_owned(), l.target)
            };
            t.map(|t| t.links.iter().map(words).collect::<Vec<_>>())
        };
        let first = links(blocks.first()).unwrap_or_default();
        assert_eq!(first, [("the first note".into(), 0), ("a site".into(), 1)]);
        let second = links(blocks.get(1)).unwrap_or_default();
        assert_eq!(second, [("Back".into(), 2), ("run".into(), 3)]);
        let inside = |s: &str| Href::Inside(s.to_owned());
        assert_eq!(
            refs.hrefs,
            [
                inside("/OEBPS/text/c2.xhtml#n 1"),
                Href::Outside("https://example.org/".into()),
                inside(&format!("{BASE}#top")),
                Href::Outside("javascript:x()".into()),
            ]
        );
    }
}
