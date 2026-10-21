//! Markup as JSON nodes: text, or `[name, {attributes}, [children]]`.

use std::cell::RefCell;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use serde_json::{Map, Value};

/// Elements nested deeper than this are read as their parent's.
const MAX_DEPTH: usize = 512;
/// Nodes read from one piece of markup; the rest are dropped.
const MAX_NODES: usize = 1 << 18;

/// Elements that never hold anything.
pub(crate) const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// `markup`'s top-level nodes, as a JSON array.
pub(crate) fn read(markup: &str) -> String {
    let sink = Sink(RefCell::new(Build {
        open: vec![Open::root()],
        nodes: 0,
    }));
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(markup));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let mut build = tokenizer.sink.0.into_inner();
    while build.open.len() > 1 {
        build.close();
    }
    let root = build.open.pop().map(|o| o.children).unwrap_or_default();
    Value::Array(root).to_string()
}

/// An element still open, and what it holds so far.
struct Open {
    name: String,
    attrs: Map<String, Value>,
    children: Vec<Value>,
}

impl Open {
    fn root() -> Self {
        Self {
            name: String::new(),
            attrs: Map::new(),
            children: Vec::new(),
        }
    }

    fn node(self) -> Value {
        let attrs = Value::Object(self.attrs);
        Value::Array(vec![
            Value::String(self.name),
            attrs,
            Value::Array(self.children),
        ])
    }
}

struct Build {
    open: Vec<Open>,
    nodes: usize,
}

impl Build {
    fn add(&mut self, node: Value) {
        if self.nodes >= MAX_NODES {
            return;
        }
        self.nodes += 1;
        if let Some(top) = self.open.last_mut() {
            top.children.push(node);
        }
    }

    fn text(&mut self, text: &str) {
        let Some(top) = self.open.last_mut() else {
            return;
        };
        // Text arrives in pieces; one string holds a run of it.
        if let Some(Value::String(last)) = top.children.last_mut() {
            last.push_str(text);
            return;
        }
        self.add(Value::String(text.to_owned()));
    }

    fn close(&mut self) {
        if self.open.len() > 1
            && let Some(done) = self.open.pop()
        {
            self.add(done.node());
        }
    }

    fn end(&mut self, name: &str) {
        let Some(at) = self.open.iter().rposition(|o| o.name == name) else {
            return;
        };
        while self.open.len() > at.max(1) {
            self.close();
        }
    }
}

struct Sink(RefCell<Build>);

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut b = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => {
                let name = tag.name.to_string();
                let mut attrs = Map::new();
                for a in &tag.attrs {
                    let value = Value::String(a.value.to_string());
                    attrs.entry(a.name.local.to_string()).or_insert(value);
                }
                let open = Open {
                    name: name.clone(),
                    attrs,
                    children: Vec::new(),
                };
                let empty = tag.self_closing || VOID.contains(&name.as_str());
                if empty || b.open.len() >= MAX_DEPTH {
                    b.add(open.node());
                    return TokenSinkResult::Continue;
                }
                b.open.push(open);
                return match name.as_str() {
                    "script" => TokenSinkResult::RawData(RawKind::ScriptData),
                    "style" | "xmp" => TokenSinkResult::RawData(RawKind::Rawtext),
                    "title" | "textarea" => TokenSinkResult::RawData(RawKind::Rcdata),
                    _ => TokenSinkResult::Continue,
                };
            }
            Token::TagToken(tag) => b.end(&tag.name),
            Token::CharacterTokens(text) => b.text(&text),
            Token::EOFToken => {
                while b.open.len() > 1 {
                    b.close();
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
    fn markup_becomes_nested_nodes() {
        let tree = read(r#"<p id="a">one <b>two</b><br/>x &amp; y</p><style>a > b {}</style>"#);
        assert_eq!(
            tree,
            r#"[["p",{"id":"a"},["one ",["b",{},["two"]],["br",{},[]],"x & y"]],["style",{},["a > b {}"]]]"#
        );
        // Unclosed elements close at the end; stray end tags are ignored.
        assert_eq!(read("<div><p>a</span>"), r#"[["div",{},[["p",{},["a"]]]]]"#);
    }
}
