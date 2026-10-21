//! MathML: formulas set with STIX Two Math, drawn as SVG pictures.
mod font;
mod layout;
mod ops;
mod paint;
mod variant;

use html5ever::tokenizer::{Tag, TagKind, Token};

use layout::{Ctx, Layout};

/// Elements one formula may hold; past it the formula falls back.
pub(crate) const MAX_NODES: usize = 4096;
/// How deep a formula may nest.
pub(crate) const MAX_NEST: usize = 48;
/// Bytes of text one formula keeps.
const MAX_TEXT: usize = 16 * 1024;
/// Attributes kept per element.
const MAX_ATTRS: usize = 32;

/// A MathML element: its name without prefix, attributes and children.
#[derive(Debug, Default, Clone)]
pub(crate) struct Node {
    pub(crate) name: String,
    pub(crate) attrs: Vec<(String, String)>,
    pub(crate) kids: Vec<Node>,
    /// Its own text, as one run.
    pub(crate) text: String,
}

impl Node {
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        let found = self.attrs.iter().find(|(n, _)| n == name);
        found.map(|(_, v)| v.as_str())
    }

    /// Its text and its children's, runs of space as one, trimmed.
    pub(crate) fn words(&self) -> String {
        let mut all = String::new();
        self.gather(&mut all, 0);
        all.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn gather(&self, out: &mut String, depth: usize) {
        out.push_str(&self.text);
        if depth < MAX_NEST {
            for k in &self.kids {
                out.push(' ');
                k.gather(out, depth + 1);
            }
        }
    }
}

/// A `math` element being read from the tokens.
#[derive(Debug, Default)]
pub(crate) struct Capture {
    open: Vec<Node>,
    root: Option<Node>,
    nodes: usize,
    text: usize,
    /// Elements past the caps, skipped to their end.
    skipping: usize,
    /// Too large or deep to set; it falls back to its text.
    pub(crate) broken: bool,
}

impl Capture {
    /// Starts at the `math` start tag `tag`.
    pub(crate) fn new(tag: &Tag) -> Self {
        let mut c = Self::default();
        c.start(tag);
        c
    }

    /// Takes the next token; `true` once the root's end tag is read.
    pub(crate) fn token(&mut self, token: &Token) -> bool {
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => self.start(tag),
            Token::TagToken(tag) => return self.end(&local(&tag.name)),
            // Indentation between elements is never shown, so never kept.
            Token::CharacterTokens(t) if t.trim().is_empty() => {}
            Token::CharacterTokens(t) => {
                let room = self.text + t.len() <= MAX_TEXT;
                match self.open.last_mut().filter(|_| self.skipping == 0) {
                    Some(top) if room => {
                        top.text.push_str(t);
                        self.text += t.len();
                    }
                    Some(_) => self.broken = true,
                    None => {}
                }
            }
            Token::EOFToken => {
                while self.close_top() {}
                return true;
            }
            _ => {}
        }
        false
    }

    fn start(&mut self, tag: &Tag) {
        let full = self.nodes >= MAX_NODES || self.open.len() >= MAX_NEST;
        if self.skipping > 0 || full {
            self.broken |= full;
            self.skipping += usize::from(!tag.self_closing);
            return;
        }
        self.nodes += 1;
        let attrs = tag.attrs.iter().take(MAX_ATTRS).map(|a| {
            let name: &str = &a.name.local;
            (name.to_ascii_lowercase(), a.value.to_string())
        });
        let node = Node {
            name: local(&tag.name),
            attrs: attrs.collect(),
            ..Node::default()
        };
        self.open.push(node);
        if tag.self_closing {
            self.close_top();
        }
    }

    fn end(&mut self, name: &str) -> bool {
        if self.skipping > 0 {
            self.skipping -= 1;
            return false;
        }
        // An end tag closes what it names and anything left open inside.
        if self.open.iter().any(|n| n.name == name) {
            while let Some(top) = self.open.last() {
                let done = top.name == name;
                self.close_top();
                if done {
                    break;
                }
            }
        }
        self.open.is_empty() && self.root.is_some()
    }

    /// Closes the innermost element; `false` once none are open.
    fn close_top(&mut self) -> bool {
        let Some(node) = self.open.pop() else {
            return false;
        };
        match self.open.last_mut() {
            Some(parent) => parent.kids.push(node),
            None => self.root = Some(node),
        }
        true
    }

    pub(crate) fn root(self) -> Option<Node> {
        self.root
    }
}

/// An element's name without its prefix, as `m:mi` is written.
fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

/// A formula drawn: SVG markup and its size in ems.
#[derive(Debug, Clone)]
pub(crate) struct Formula {
    pub(crate) svg: String,
    pub(crate) width: f32,
    pub(crate) height: f32,
    /// How far below the baseline it reaches.
    pub(crate) depth: f32,
}

/// Whether `root` is set as its own block, centred.
pub(crate) fn is_block(root: &Node) -> bool {
    let attr = |name: &str| root.attr(name).map(str::trim);
    attr("display") == Some("block") || attr("mode") == Some("display")
}

/// `root` set and drawn; `None` if it holds nothing that can be shown.
pub(crate) fn render(root: &Node, block: bool) -> Option<Formula> {
    let f = font::font()?;
    let mut layout = Layout::new(f);
    let b = layout.lay(root, Ctx::new(block), 0);
    if layout.full || b.items.is_empty() || b.w <= 0.0 || b.a + b.d <= 0.0 {
        return None;
    }
    let svg = paint::svg(&b, f)?;
    Some(Formula {
        svg,
        width: b.w + 2.0 * paint::PAD,
        height: b.a + b.d + 2.0 * paint::PAD,
        depth: b.d + paint::PAD,
    })
}

/// What stands in for an undrawn `root`: its description, else its text.
pub(crate) fn alt(root: &Node) -> Option<String> {
    let given = root
        .attr("alttext")
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let text = given.map_or_else(|| root.words(), str::to_owned);
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;

    use html5ever::tendril::StrTendril;
    use html5ever::tokenizer::{BufferQueue, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};

    use super::*;

    #[derive(Default)]
    struct Catch(RefCell<(Option<Capture>, bool)>);

    impl TokenSink for Catch {
        type Handle = ();

        fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
            let mut s = self.0.borrow_mut();
            let (capture, done) = &mut *s;
            if *done {
                return TokenSinkResult::Continue;
            }
            match capture {
                Some(c) => *done = c.token(&token),
                None => {
                    if let Token::TagToken(tag) = &token
                        && tag.kind == TagKind::StartTag
                        && local(&tag.name) == "math"
                    {
                        *capture = Some(Capture::new(tag));
                    }
                }
            }
            TokenSinkResult::Continue
        }
    }

    /// The first formula in `markup`, read as the chapter reader does.
    pub(crate) fn tree(markup: &str) -> Option<Node> {
        let tokenizer = Tokenizer::new(Catch::default(), TokenizerOpts::default());
        let input = BufferQueue::default();
        input.push_back(StrTendril::from_slice(markup));
        let _ = tokenizer.feed(&input);
        tokenizer.end();
        let (capture, _) = tokenizer.sink.0.into_inner();
        capture?.root()
    }

    #[test]
    fn tokens_build_the_tree() {
        let root =
            tree("<m:math display='block'><m:mi>x</m:mi><m:mo>=</m:mo><m:mn>2</m:mn></m:math>");
        let root = root.expect("a tree");
        assert_eq!(root.name, "math");
        assert!(is_block(&root));
        let names: Vec<&str> = root.kids.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["mi", "mo", "mn"]);
        assert_eq!(root.words(), "x = 2");
    }

    #[test]
    fn a_formula_draws_as_svg() {
        let root = tree("<math><msup><mi>e</mi><mrow><mi>i</mi><mi>π</mi></mrow></msup></math>");
        let f = root.as_ref().and_then(|r| render(r, false));
        let f = f.expect("drawn");
        assert!(f.svg.starts_with("<svg") && f.svg.contains("<path"));
        assert!(f.width > 0.8 && f.height > 0.6 && f.depth > 0.0);
    }

    #[test]
    fn deep_or_huge_formulas_break_cleanly() {
        let deep = format!(
            "<math>{}<mi>x</mi>{}</math>",
            "<mrow>".repeat(200),
            "</mrow>".repeat(200)
        );
        let tokenizer = Tokenizer::new(Catch::default(), TokenizerOpts::default());
        let input = BufferQueue::default();
        input.push_back(StrTendril::from_slice(&deep));
        let _ = tokenizer.feed(&input);
        tokenizer.end();
        let (capture, done) = tokenizer.sink.0.into_inner();
        assert!(done, "the end tag is still found");
        assert!(capture.is_some_and(|c| c.broken));
    }

    #[test]
    fn indentation_never_counts_against_the_caps() {
        let pad = format!("\r\n{}", " ".repeat(200));
        let rows = format!("<mtr><mtd>{pad}<mi>x</mi>{pad}</mtd></mtr>").repeat(120);
        let root = tree(&format!("<math><mtable>{rows}</mtable></math>"));
        let f = root.as_ref().and_then(|r| render(r, true));
        assert!(f.is_some_and(|f| f.height > 20.0));
    }

    #[test]
    fn alt_prefers_the_description() {
        let root = tree("<math alttext='x squared'><msup><mi>x</mi><mn>2</mn></msup></math>");
        assert_eq!(root.as_ref().and_then(alt).as_deref(), Some("x squared"));
        let plain = tree("<math><mi>a</mi><mo>+</mo><mi>b</mi></math>");
        assert_eq!(plain.as_ref().and_then(alt).as_deref(), Some("a + b"));
    }
}
