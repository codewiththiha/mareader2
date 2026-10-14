//! Chapter XHTML to blocks: html5ever's tokenizer and our tag stack, no DOM.
use std::cell::RefCell;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use mareader_reflow::{Block, Emphasis, Role};

/// Open elements tracked at once; deeper nesting is read as flat.
pub const MAX_DEPTH: usize = 256;

/// What an open element does to the text inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Effect {
    Italic,
    Bold,
    /// Not shown: the head, scripts, styles, pictures drawn in markup.
    Hidden,
}

fn effect(name: &str) -> Option<Effect> {
    match name {
        "em" | "i" | "cite" | "var" | "dfn" => Some(Effect::Italic),
        "strong" | "b" => Some(Effect::Bold),
        "head" | "style" | "script" | "title" | "svg" | "math" | "template" | "noscript" => {
            Some(Effect::Hidden)
        }
        _ => None,
    }
}

/// Elements that end the block before them and start a new one.
fn block_role(name: &str) -> Option<Role> {
    let heading = |n: u8| Some(Role::Heading(n));
    match name {
        "h1" => heading(1),
        "h2" => heading(2),
        "h3" => heading(3),
        "h4" => heading(4),
        "h5" => heading(5),
        "h6" => heading(6),
        "p" | "div" | "li" | "blockquote" | "pre" | "section" | "article" | "header" | "footer"
        | "aside" | "nav" | "figure" | "figcaption" | "dd" | "dt" | "tr" | "td" | "th"
        | "table" | "ul" | "ol" | "dl" | "hr" | "body" | "main" | "address" | "caption" => {
            Some(Role::Paragraph)
        }
        _ => None,
    }
}

/// HTML's collapsible spaces; U+00A0 and friends stay as written.
fn is_html_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

#[derive(Default)]
struct State {
    blocks: Vec<Block>,
    current: Option<Block>,
    /// Open elements with an effect, innermost last.
    stack: Vec<(String, Effect)>,
    /// A collapsed space waits for the next visible character.
    space: bool,
    /// The next block opens a chapter: it starts a page.
    chapter: bool,
    text: String,
}

impl State {
    fn has(&self, e: Effect) -> bool {
        self.stack.iter().any(|(_, x)| *x == e)
    }

    fn emphasis(&self) -> Emphasis {
        Emphasis {
            italic: self.has(Effect::Italic),
            bold: self.has(Effect::Bold),
        }
    }

    fn finish(&mut self) {
        if let Some(mut block) = self.current.take() {
            block.trim_end();
            if !block.is_blank() {
                block.break_before = std::mem::take(&mut self.chapter);
                self.blocks.push(block);
            }
        }
        self.space = false;
    }

    fn start(&mut self, role: Role) {
        self.finish();
        self.current = Some(Block::new(role));
    }

    /// Collapses spaces as HTML does, then adds the text.
    fn characters(&mut self, input: &str) {
        if self.has(Effect::Hidden) {
            return;
        }
        let emphasis = self.emphasis();
        let block = self
            .current
            .get_or_insert_with(|| Block::new(Role::Paragraph));
        self.text.clear();
        for c in input.chars() {
            if is_html_space(c) {
                self.space = true;
                continue;
            }
            let line_start =
                self.text.is_empty() && (block.text.is_empty() || block.text.ends_with('\n'));
            if std::mem::take(&mut self.space) && !line_start {
                self.text.push(' ');
            }
            self.text.push(c);
        }
        block.push(&self.text, emphasis);
    }

    fn line_break(&mut self) {
        if let Some(block) = self.current.as_mut().filter(|b| !b.text.is_empty()) {
            block.push("\n", Emphasis::default());
        }
        self.space = false;
    }

    /// Returns whether the tokenizer should read raw text next.
    fn tag(&mut self, tag: &Tag) -> bool {
        let name: &str = &tag.name;
        match tag.kind {
            TagKind::StartTag => {
                if name == "br" {
                    self.line_break();
                    return false;
                }
                if let Some(role) = block_role(name) {
                    self.start(role);
                }
                let Some(e) = effect(name) else {
                    return false;
                };
                // `<script/>` and `<a id="x"/>` open nothing.
                if tag.self_closing || self.stack.len() >= MAX_DEPTH {
                    return false;
                }
                self.stack.push((name.to_owned(), e));
                matches!(name, "style" | "script" | "title" | "noscript" | "template")
            }
            TagKind::EndTag => {
                if block_role(name).is_some() {
                    self.finish();
                }
                // Close the innermost match and anything left open in it.
                if let Some(at) = self.stack.iter().rposition(|(n, _)| n == name) {
                    self.stack.truncate(at);
                }
                false
            }
        }
    }
}

struct Sink(RefCell<State>);

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut s = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                if s.tag(&tag) {
                    return TokenSinkResult::RawData(RawKind::Rawtext);
                }
            }
            Token::CharacterTokens(text) => s.characters(&text),
            Token::EOFToken => s.finish(),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

/// Appends `xhtml`'s blocks; the first starts a new page.
pub fn read_chapter(xhtml: &str, out: &mut Vec<Block>) {
    let sink = Sink(RefCell::new(State {
        chapter: true,
        ..State::default()
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
    use super::*;

    fn read(xhtml: &str) -> Vec<Block> {
        let mut out = Vec::new();
        read_chapter(xhtml, &mut out);
        out
    }

    fn texts(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().map(|b| b.text.as_str()).collect()
    }

    #[test]
    fn headings_and_paragraphs_become_blocks() {
        let b = read(
            "<?xml version='1.0'?><html><head><title>T</title>\
             <style>p { color: red }</style></head><body>\
             <h2>Chapter  One</h2><p>First\n   para.</p><p>Second.</p></body></html>",
        );
        assert_eq!(texts(&b), ["Chapter One", "First para.", "Second."]);
        assert_eq!(b.first().map(|b| b.role), Some(Role::Heading(2)));
        assert_eq!(b.get(1).map(|b| b.role), Some(Role::Paragraph));
    }

    #[test]
    fn only_the_first_block_starts_a_page() {
        let b = read("<p>a</p><p>b</p>");
        let breaks: Vec<bool> = b.iter().map(|b| b.break_before).collect();
        assert_eq!(breaks, [true, false]);
    }

    #[test]
    fn emphasis_marks_spans() {
        let b = read("<p>a <em>b <strong>c</strong></em> d</p>");
        let p = b.first();
        assert_eq!(p.map(|p| p.text.as_str()), Some("a b c d"));
        let spans: Vec<(&str, Emphasis)> = p
            .map(|p| {
                p.spans
                    .iter()
                    .filter_map(|s| Some((p.text.get(s.range.clone())?, s.emphasis)))
                    .collect()
            })
            .unwrap_or_default();
        let it = |italic, bold| Emphasis { italic, bold };
        // A collapsed space joins the run after it.
        assert_eq!(spans, [(" b", it(true, false)), (" c", it(true, true))]);
    }

    #[test]
    fn self_closing_tags_open_nothing() {
        let b = read(
            "<head><script src='x.js'/><link rel='stylesheet'/></head>\
             <p><a id='n1'/>Kept <em/>plain.</p>",
        );
        assert_eq!(texts(&b), ["Kept plain."]);
        assert!(b.first().is_some_and(|p| p.spans.is_empty()));
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
}
