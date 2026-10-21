//! Scripted chapters: their scripts, sheets, and links into their pages.

use std::cell::RefCell;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};

use crate::href;

/// The attribute a scripted page marks what responds to clicks with.
pub(crate) const NODE: &str = "data-mareader-node";
/// Link targets into scripted pages: the top bit, a chapter, a node.
const SCRIPTED: u32 = 1 << 31;
const NODE_BITS: u32 = 20;
const CHAPTERS: usize = 1 << 11;
/// Scripts read from one chapter, and code kept per book.
const MAX_SCRIPTS: usize = 256;
pub(crate) const MAX_CODE: usize = 4 << 20;

/// A script as its chapter wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Code {
    Inline(String),
    /// A file in the archive, its path resolved.
    File(String),
}

/// What a chapter's scripts need besides its markup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) code: Vec<Code>,
    /// Linked style sheets' paths, resolved.
    pub(crate) styles: Vec<String>,
}

/// A scripted chapter, ready to hand to a script engine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scripted {
    /// What [`crate::Destination::Script`] and [`crate::Book::relaid`] call it.
    pub chapter: usize,
    pub path: String,
    pub xhtml: String,
    /// Its scripts' code in document order; files that won't read are left out.
    pub code: Vec<String>,
    /// Its linked style sheets' text.
    pub styles: Vec<String>,
}

/// The link target for node `node` of scripted chapter `chapter`.
pub(crate) fn link(chapter: usize, node: u32) -> Option<u32> {
    let chapter = u32::try_from(chapter).ok().filter(|_| chapter < CHAPTERS)?;
    (node < 1 << NODE_BITS).then_some(SCRIPTED | chapter << NODE_BITS | node)
}

/// The chapter and node a link target names, if it is a scripted one.
pub(crate) fn unlink(target: u32) -> Option<(usize, u32)> {
    if target & SCRIPTED == 0 {
        return None;
    }
    let chapter = (target & !SCRIPTED) >> NODE_BITS;
    let node = target & ((1 << NODE_BITS) - 1);
    Some((usize::try_from(chapter).ok()?, node))
}

/// The scripts in `xhtml`, at `base`, and the sheets it links.
pub(crate) fn find(xhtml: &str, base: &str) -> Found {
    if !xhtml.contains("<script") && !xhtml.contains("<SCRIPT") {
        return Found::default();
    }
    let sink = Scan(RefCell::new(Scanning {
        base,
        found: Found::default(),
        open: None,
    }));
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(xhtml));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let found = tokenizer.sink.0.into_inner().found;
    match found.code.is_empty() {
        true => Found::default(),
        false => found,
    }
}

/// Whether a script of type `kind` is JavaScript; none means it is.
fn javascript(kind: Option<&str>) -> bool {
    let kind = kind
        .map(|k| k.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let kind = kind.split(';').next().unwrap_or_default().trim();
    matches!(
        kind,
        "" | "text/javascript"
            | "application/javascript"
            | "application/ecmascript"
            | "text/ecmascript"
            | "application/x-javascript"
            | "module"
    )
}

fn attr<'t>(tag: &'t Tag, name: &str) -> Option<&'t str> {
    let found = tag.attrs.iter().find(|a| &*a.name.local == name);
    found.map(|a| a.value.trim()).filter(|v| !v.is_empty())
}

struct Scanning<'a> {
    base: &'a str,
    found: Found,
    /// Inline code read so far, while in a script.
    open: Option<String>,
}

struct Scan<'a>(RefCell<Scanning<'a>>);

impl TokenSink for Scan<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut s = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => match &*tag.name {
                "script" if s.found.code.len() < MAX_SCRIPTS => {
                    if !javascript(attr(&tag, "type")) {
                        return raw(&tag);
                    }
                    if let Some(src) = attr(&tag, "src") {
                        if let Some(path) = href::resolve(s.base, src) {
                            s.found.code.push(Code::File(path));
                        }
                        return raw(&tag);
                    }
                    s.open = Some(String::new());
                    return raw(&tag);
                }
                "script" | "style" => return raw(&tag),
                "link" => {
                    let rel = attr(&tag, "rel").unwrap_or_default().to_ascii_lowercase();
                    let sheet = rel.split_whitespace().any(|r| r == "stylesheet");
                    let alternate = rel.split_whitespace().any(|r| r == "alternate");
                    let href = attr(&tag, "href").filter(|_| sheet && !alternate);
                    if let Some(path) = href.and_then(|h| href::resolve(s.base, h))
                        && s.found.styles.len() < MAX_SCRIPTS
                    {
                        s.found.styles.push(path);
                    }
                }
                _ => {}
            },
            Token::TagToken(tag) if &*tag.name == "script" => {
                if let Some(code) = s.open.take() {
                    s.found.code.push(Code::Inline(code));
                }
            }
            Token::CharacterTokens(text) => {
                if let Some(code) = s.open.as_mut()
                    && code.len() < MAX_CODE
                {
                    code.push_str(&text);
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn raw(tag: &Tag) -> TokenSinkResult<()> {
    match tag.self_closing {
        true => TokenSinkResult::Continue,
        false => TokenSinkResult::RawData(RawKind::ScriptData),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_and_sheets_are_found_in_order() {
        let xhtml = r#"<html><head><link rel="stylesheet" href="../css/a.css"/>
            <script src="../js/q.js"></script><script type="text/x-template">no</script>
            <script>if (a < b) go();</script></head><body/></html>"#;
        let found = find(xhtml, "OEBPS/text/c1.xhtml");
        assert_eq!(found.styles, ["/OEBPS/css/a.css"]);
        let code = vec![
            Code::File("/OEBPS/js/q.js".into()),
            Code::Inline("if (a < b) go();".into()),
        ];
        assert_eq!(found.code, code);
        assert_eq!(find("<p>plain</p>", "a.xhtml"), Found::default());
    }

    #[test]
    fn links_carry_their_chapter_and_node() {
        let t = link(5, 77).expect("fits");
        assert_eq!(unlink(t), Some((5, 77)));
        assert_eq!(unlink(12), None);
        assert_eq!(link(CHAPTERS, 1), None);
        assert_eq!(link(1, 1 << NODE_BITS), None);
    }
}
