//! A CSS subset: tag and class rules for emphasis, alignment and hiding.
use std::collections::HashMap;

use mareader_reflow::Align;

/// A stylesheet's text past this is ignored.
pub const MAX_SHEET: usize = 1 << 20;
const MAX_RULES: usize = 10_000;

/// The properties read; `None` leaves the inherited value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Decl {
    pub(crate) italic: Option<bool>,
    pub(crate) bold: Option<bool>,
    pub(crate) align: Option<Align>,
    pub(crate) hidden: Option<bool>,
}

impl Decl {
    /// A declaration block, as in a rule or a `style` attribute.
    pub(crate) fn parse(text: &str) -> Self {
        let mut d = Self::default();
        for item in text.split(';') {
            let Some((name, value)) = item.split_once(':') else {
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.to_ascii_lowercase();
            let value = value.trim().trim_end_matches("!important").trim();
            match name.as_str() {
                "font-style" => d.italic = italic(value).or(d.italic),
                "font-weight" => d.bold = bold(value).or(d.bold),
                "text-align" => d.align = align(value).or(d.align),
                "display" => d.hidden = Some(value == "none"),
                "font" => {
                    let words = || value.split_ascii_whitespace();
                    d.italic = words().find_map(italic).or(d.italic);
                    d.bold = words().find_map(bold).or(d.bold);
                }
                _ => {}
            }
        }
        d
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// `self` over `under`: what `self` leaves unset comes from `under`.
    fn over(self, under: Self) -> Self {
        Self {
            italic: self.italic.or(under.italic),
            bold: self.bold.or(under.bold),
            align: self.align.or(under.align),
            hidden: self.hidden.or(under.hidden),
        }
    }
}

fn italic(v: &str) -> Option<bool> {
    match v {
        "italic" | "oblique" => Some(true),
        "normal" => Some(false),
        _ => None,
    }
}

fn bold(v: &str) -> Option<bool> {
    match v {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        _ => v.parse::<u32>().ok().map(|w| w >= 600),
    }
}

/// Also the HTML `align` attribute's values.
pub(crate) fn align(v: &str) -> Option<Align> {
    match v.trim().to_ascii_lowercase().as_str() {
        "center" | "-webkit-center" => Some(Align::Center),
        "right" | "end" => Some(Align::End),
        // Justification comes with hyphenation, later.
        "left" | "start" | "justify" => Some(Align::Start),
        _ => None,
    }
}

/// `tag`, `.class` or `tag.class`; specificity orders them.
struct Rule {
    tag: Option<String>,
    class: Option<String>,
    decl: Decl,
    /// Where it last appeared among the sheet's rules.
    order: usize,
}

impl Rule {
    fn specificity(&self) -> u8 {
        u8::from(self.tag.is_some()) + 10 * u8::from(self.class.is_some())
    }
}

/// One stylesheet's usable rules, one per selector, indexed by it.
#[derive(Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
    by_tag: HashMap<String, usize>,
    by_class: HashMap<String, ByClass>,
    /// Rules seen, repeats included.
    seen: usize,
}

/// A class's rules: `.class`, and `tag.class` by tag.
#[derive(Default)]
struct ByClass {
    any: Option<usize>,
    tags: HashMap<String, usize>,
}

impl std::fmt::Debug for Stylesheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stylesheet({} rules)", self.rules.len())
    }
}

impl Stylesheet {
    /// Keeps simple selectors; @-rules and the rest are skipped whole.
    pub(crate) fn parse(css: &str) -> Self {
        let mut sheet = Self::default();
        let css = strip_comments(css.get(..floor(css, MAX_SHEET)).unwrap_or_default());
        let mut rest = css.as_str();
        while let Some(open) = rest.find('{') {
            // Statements like `@import x;` end before the selector.
            let prelude = rest.get(..open).unwrap_or_default();
            let prelude = prelude.rsplit(';').next().unwrap_or_default().trim();
            let body = rest.get(open + 1..).unwrap_or_default();
            let Some(close) = matching_brace(body) else {
                break;
            };
            if !prelude.starts_with('@') {
                let decl = Decl::parse(body.get(..close).unwrap_or_default());
                if !decl.is_empty() {
                    for selector in prelude.split(',') {
                        sheet.add(selector, decl);
                    }
                }
            }
            rest = body.get(close + 1..).unwrap_or_default();
        }
        sheet
    }

    /// A repeated selector merges into its rule, later values winning.
    fn add(&mut self, selector: &str, decl: Decl) {
        let Some((tag, class)) = simple_selector(selector) else {
            return;
        };
        let order = self.seen;
        self.seen += 1;
        let slot = match (&tag, &class) {
            (Some(t), None) => self.by_tag.get(t),
            (None, Some(c)) => self.by_class.get(c).and_then(|b| b.any.as_ref()),
            (Some(t), Some(c)) => self.by_class.get(c).and_then(|b| b.tags.get(t)),
            (None, None) => return,
        };
        if let Some(rule) = slot.copied().and_then(|at| self.rules.get_mut(at)) {
            rule.decl = decl.over(rule.decl);
            rule.order = order;
            return;
        }
        if self.rules.len() >= MAX_RULES {
            return;
        }
        let at = self.rules.len();
        match (&tag, &class) {
            (Some(t), None) => {
                self.by_tag.insert(t.clone(), at);
            }
            (None, Some(c)) => self.by_class.entry(c.clone()).or_default().any = Some(at),
            (Some(t), Some(c)) => {
                let by = self.by_class.entry(c.clone()).or_default();
                by.tags.insert(t.clone(), at);
            }
            (None, None) => return,
        }
        let rule = Rule {
            tag,
            class,
            decl,
            order,
        };
        self.rules.push(rule);
    }

    /// Calls `found(specificity, order, decl)` for each matching rule.
    pub(crate) fn matching(
        &self,
        tag: &str,
        classes: &[&str],
        found: &mut impl FnMut(u8, usize, Decl),
    ) {
        let by_class = classes.iter().filter_map(|c| self.by_class.get(*c));
        let by_class = by_class.flat_map(|b| b.any.into_iter().chain(b.tags.get(tag).copied()));
        for at in self.by_tag.get(tag).copied().into_iter().chain(by_class) {
            if let Some(rule) = self.rules.get(at) {
                found(rule.specificity(), rule.order, rule.decl);
            }
        }
    }
}

/// The largest char boundary at or below `max`.
fn floor(s: &str, max: usize) -> usize {
    (0..=max.min(s.len()))
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0)
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(rest.get(..start).unwrap_or_default());
        let after = rest.get(start + 2..).unwrap_or_default();
        rest = after
            .find("*/")
            .map_or("", |end| after.get(end + 2..).unwrap_or_default());
    }
    out.push_str(rest);
    out
}

/// Where the block opened just before `body` closes, nesting included.
fn matching_brace(body: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn simple_selector(s: &str) -> Option<(Option<String>, Option<String>)> {
    let s = s.trim();
    let (tag, class) = match s.split_once('.') {
        Some((t, c)) => (t, Some(c)),
        None => (s, None),
    };
    let ident = |x: &str| {
        !x.is_empty()
            && x.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if !(tag.is_empty() || ident(tag)) || class.is_some_and(|c| !ident(c)) {
        return None;
    }
    let tag = (!tag.is_empty()).then(|| tag.to_ascii_lowercase());
    Some((tag, class.map(str::to_owned)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decls(sheet: &Stylesheet, tag: &str, classes: &[&str]) -> Vec<Decl> {
        let mut found = Vec::new();
        sheet.matching(tag, classes, &mut |spec, at, d| found.push((spec, at, d)));
        found.sort_by_key(|f| (f.0, f.1));
        found.into_iter().map(|f| f.2).collect()
    }

    #[test]
    fn declarations_read_the_supported_properties() {
        let d = Decl::parse("font-style: italic; FONT-WEIGHT: 700 !important; text-align:center");
        assert_eq!(d.italic, Some(true));
        assert_eq!(d.bold, Some(true));
        assert_eq!(d.align, Some(Align::Center));
        assert_eq!(Decl::parse("display: none").hidden, Some(true));
        assert_eq!(Decl::parse("font-weight: 400").bold, Some(false));
        assert_eq!(
            Decl::parse("font: italic bold 1em serif").italic,
            Some(true)
        );
        assert!(Decl::parse("color: red; margin: 0").is_empty());
    }

    #[test]
    fn simple_selectors_match_and_the_rest_are_skipped() {
        let sheet = Stylesheet::parse(
            "@charset 'utf-8'; /* a { font-style: italic } */\n\
             p { text-align: justify }\n\
             .centre, h1.title { text-align: center }\n\
             div p, a:hover, #id, [x] { font-weight: bold }\n\
             @media screen { p { font-style: italic } }\n\
             p.note { font-style: italic }",
        );
        let align = |tag, classes: &[&str]| {
            decls(&sheet, tag, classes)
                .iter()
                .filter_map(|d| d.align)
                .next_back()
        };
        assert_eq!(align("p", &[]), Some(Align::Start));
        assert_eq!(align("p", &["centre"]), Some(Align::Center));
        assert_eq!(align("h1", &["title"]), Some(Align::Center));
        assert_eq!(align("h2", &["title"]), None);
        // Nothing from the comment, the compound selectors or @media.
        assert_eq!(
            decls(&sheet, "p", &[])
                .iter()
                .filter(|d| d.italic.is_some())
                .count(),
            0
        );
        assert!(decls(&sheet, "a", &[]).is_empty());
        assert_eq!(
            decls(&sheet, "p", &["note"]).last().and_then(|d| d.italic),
            Some(true)
        );
    }

    #[test]
    fn junk_and_unclosed_rules_end_quietly() {
        let sheet = Stylesheet::parse("p { font-style: italic; } q { font-weight");
        assert_eq!(decls(&sheet, "p", &[]).len(), 1);
        assert!(decls(&sheet, "q", &[]).is_empty());
        let _ = Stylesheet::parse("}}}{{{ ; @ { /* ");
    }

    #[test]
    fn repeated_selectors_merge_so_each_element_meets_few_rules() {
        let sheet = Stylesheet::parse(
            "p { font-style: italic; text-align: center } .x { font-weight: bold }\n\
             p { text-align: right }",
        );
        let found = decls(&sheet, "p", &["x"]);
        // The two `p` rules merge, the later alignment winning.
        assert_eq!(found.len(), 2);
        let p = found.first().copied().unwrap_or_default();
        assert_eq!((p.italic, p.align), (Some(true), Some(Align::End)));
        // Thousands of rules, but one per selector an element can match.
        let mut css = "p { font-style: italic }".repeat(5000);
        css.extend((0..5000).map(|i| format!("t{i}.c {{ font-weight: bold }}")));
        let sheet = Stylesheet::parse(&css);
        assert_eq!(decls(&sheet, "p", &["c"]).len(), 1);
        assert_eq!(decls(&sheet, "t7", &["c"]).len(), 1);
    }
}
