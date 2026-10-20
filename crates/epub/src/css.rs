//! A CSS cascade: selectors, specificity, inheritance, computed lengths.
//! See `docs/epub-layout.md`.
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use mareader_reflow::{Align, VAlign};

use crate::href;

/// A stylesheet's text past this is ignored.
pub const MAX_SHEET: usize = 1 << 20;
const MAX_RULES: usize = 10_000;
/// Compounds in one selector; longer ones are skipped.
const MAX_PARTS: usize = 16;
const MAX_FACES: usize = 256;
/// What `1rem` and the root's text measure, as browsers default it.
pub(crate) const ROOT_PX: f32 = 16.0;
/// What `vh` and `vw` measure: the page's text box, near enough.
const VIEW_HEIGHT: f32 = 980.0;
const VIEW_WIDTH: f32 = 650.0;
/// Percent widths of padding and margins: a column, near enough.
pub(crate) const BASE_WIDTH: f32 = 600.0;

/// A length as written; `Em` is of the element's own text size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Len {
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    Vh(f32),
    Vw(f32),
    Auto,
}

impl Len {
    /// In px: `em` is the text size, `base` what 100% is.
    pub(crate) fn px(self, em: f32, base: f32) -> Option<f32> {
        Some(match self {
            Self::Px(n) => n,
            Self::Em(n) => n * em,
            Self::Rem(n) => n * ROOT_PX,
            Self::Percent(n) => n * base / 100.0,
            Self::Vh(n) => n * VIEW_HEIGHT / 100.0,
            Self::Vw(n) => n * VIEW_WIDTH / 100.0,
            Self::Auto => return None,
        })
    }

    /// Computed: absolute units become px; percentages and auto stay.
    fn computed(self, em: f32) -> Self {
        match self {
            Self::Percent(_) | Self::Vw(_) | Self::Auto => self,
            other => Self::Px(other.px(em, 0.0).unwrap_or(0.0)),
        }
    }
}

/// Any CSS length: absolute units, font units, percent and viewport units.
pub(crate) fn len(v: &str) -> Option<Len> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("auto") {
        return Some(Len::Auto);
    }
    let numeric = |c: char| c.is_ascii_digit() || matches!(c, '.' | '-' | '+');
    let split = v.find(|c: char| !numeric(c)).unwrap_or(v.len());
    let (number, unit) = v.split_at(split);
    let n: f32 = number.parse().ok().filter(|n: &f32| n.is_finite())?;
    let n = n.clamp(-10_000.0, 10_000.0);
    Some(match unit.trim().to_ascii_lowercase().as_str() {
        "" if n == 0.0 => Len::Px(0.0),
        "px" => Len::Px(n),
        "pt" => Len::Px(n * 4.0 / 3.0),
        "pc" => Len::Px(n * 16.0),
        "in" => Len::Px(n * 96.0),
        "cm" => Len::Px(n * 96.0 / 2.54),
        "mm" => Len::Px(n * 96.0 / 25.4),
        "q" => Len::Px(n * 96.0 / 101.6),
        "em" => Len::Em(n),
        "ex" | "ch" => Len::Em(n / 2.0),
        "rem" => Len::Rem(n),
        "%" => Len::Percent(n),
        // Pages stand upright: the short side is the width.
        "vh" | "vmax" => Len::Vh(n),
        "vw" | "vmin" => Len::Vw(n),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Weight {
    Abs(u16),
    Bolder,
    Lighter,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

impl TextAlign {
    pub(crate) fn align(self) -> Align {
        match self {
            Self::Start | Self::Justify => Align::Start,
            Self::Center => Align::Center,
            Self::End => Align::End,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Display {
    #[default]
    Inline,
    Block,
    None,
    Flex,
    Grid,
    Table,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FontSize {
    Len(Len),
    Larger,
    Smaller,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LineHeight {
    Normal,
    Factor(f32),
    Len(Len),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum BorderStyle {
    #[default]
    None,
    Solid,
    Double,
    Dashed,
    Dotted,
}

/// The properties read; `None` keeps what cascades or inherits.
/// Sides go top, right, bottom, left.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Decl {
    pub(crate) italic: Option<bool>,
    pub(crate) weight: Option<Weight>,
    pub(crate) align: Option<TextAlign>,
    pub(crate) display: Option<Display>,
    /// `list-style: none`: list items show no marker.
    pub(crate) unmarked: Option<bool>,
    pub(crate) width: Option<Len>,
    pub(crate) height: Option<Len>,
    pub(crate) size: Option<FontSize>,
    pub(crate) line: Option<LineHeight>,
    pub(crate) letter: Option<Len>,
    pub(crate) word: Option<Len>,
    pub(crate) indent: Option<Len>,
    pub(crate) family: Option<Arc<[String]>>,
    pub(crate) margin: [Option<Len>; 4],
    pub(crate) padding: [Option<Len>; 4],
    pub(crate) border_width: [Option<Len>; 4],
    pub(crate) border_style: [Option<BorderStyle>; 4],
    pub(crate) background: Option<bool>,
    pub(crate) background_image: Option<bool>,
    pub(crate) float: Option<bool>,
    pub(crate) break_before: Option<bool>,
    pub(crate) valign: Option<VAlign>,
}

/// Sets each field `top` sets.
macro_rules! layer {
    ($self:ident, $top:ident; $($f:ident),*; $($side:ident),*) => {
        $(if $top.$f.is_some() { $self.$f = $top.$f.clone(); })*
        $(for (mine, theirs) in $self.$side.iter_mut().zip(&$top.$side) {
            if theirs.is_some() { *mine = *theirs; }
        })*
    };
}

impl Decl {
    /// A declaration block, `!important` ones folded over the rest.
    pub(crate) fn parse(text: &str) -> Self {
        let (mut normal, important) = Self::parse_both(text);
        normal.layer(&important);
        normal
    }

    /// A declaration block as its normal and `!important` halves.
    pub(crate) fn parse_both(text: &str) -> (Self, Self) {
        let (mut normal, mut important) = (Self::default(), Self::default());
        for item in split_top(text, ';') {
            let Some((name, value)) = item.split_once(':') else {
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            let lower = value.to_ascii_lowercase();
            let (value, strong) = match lower.find("!important") {
                Some(at) => (value.get(..at).unwrap_or_default().trim(), true),
                None => (value, false),
            };
            let target = if strong { &mut important } else { &mut normal };
            target.set(&name, value);
        }
        (normal, important)
    }

    fn set(&mut self, name: &str, value: &str) {
        let v = value.to_ascii_lowercase();
        let v = v.as_str();
        if matches!(v, "inherit" | "initial" | "unset" | "revert") {
            return;
        }
        match name {
            "font-style" => self.italic = italic(v).or(self.italic),
            "font-weight" => self.weight = weight(v).or(self.weight),
            "text-align" => self.align = text_align(v).or(self.align),
            "display" => self.display = Some(display(v)),
            "width" => self.width = len(v).or(self.width),
            "height" => self.height = len(v).or(self.height),
            "font-size" => self.size = font_size(v).or(self.size),
            "line-height" => self.line = line_height(v).or(self.line),
            "letter-spacing" => self.letter = spacing(v).or(self.letter),
            "word-spacing" => self.word = spacing(v).or(self.word),
            "text-indent" => self.indent = len(v).or(self.indent),
            "font-family" => self.family = Some(families(value)),
            "font" => self.font(value),
            "list-style" | "list-style-type" => {
                self.unmarked = Some(v.split_ascii_whitespace().any(|w| w == "none"));
            }
            "margin" => sides(&mut self.margin, v, len),
            "padding" => sides(&mut self.padding, v, len),
            "border-width" => sides(&mut self.border_width, v, border_width),
            "border-style" => sides(&mut self.border_style, v, border_style),
            "border" => self.border([true; 4], v),
            "background-color" => self.background = Some(!clear(v)),
            "background-image" => self.background_image = Some(v.contains("url(")),
            "background" => {
                self.background_image = Some(v.contains("url("));
                self.background = Some(v.split_ascii_whitespace().any(is_colour));
            }
            "float" => self.float = Some(matches!(v, "left" | "right")),
            "vertical-align" => {
                self.valign = match v {
                    "top" | "text-top" | "baseline" => Some(VAlign::Top),
                    "middle" => Some(VAlign::Middle),
                    "bottom" | "text-bottom" => Some(VAlign::Bottom),
                    _ => self.valign,
                }
            }
            "page-break-before" | "break-before" => {
                let page = ["always", "page", "left", "right", "recto", "verso"];
                self.break_before = Some(page.contains(&v));
            }
            _ => self.side(name, v),
        }
    }

    /// `margin-top`, `border-left-width` and the like.
    fn side(&mut self, name: &str, v: &str) {
        let (prop, rest) = name.split_once('-').unwrap_or((name, ""));
        let (side, part) = rest.split_once('-').unwrap_or((rest, ""));
        let Some(i) = ["top", "right", "bottom", "left"]
            .iter()
            .position(|s| *s == side)
        else {
            return;
        };
        match (prop, part) {
            ("margin", "") => set_side(&mut self.margin, i, len(v)),
            ("padding", "") => set_side(&mut self.padding, i, len(v)),
            ("border", "") => {
                let mut only = [false; 4];
                if let Some(s) = only.get_mut(i) {
                    *s = true;
                }
                self.border(only, v);
            }
            ("border", "width") => set_side(&mut self.border_width, i, border_width(v)),
            ("border", "style") => set_side(&mut self.border_style, i, border_style(v)),
            _ => {}
        }
    }

    /// The `border` shorthand on the sides in `which`; unsaid parts reset.
    fn border(&mut self, which: [bool; 4], v: &str) {
        let words = || v.split_ascii_whitespace();
        let width = words().find_map(border_width).unwrap_or(Len::Px(3.0));
        let style = words().find_map(border_style).unwrap_or_default();
        for (i, on) in which.into_iter().enumerate() {
            if on {
                set_side(&mut self.border_width, i, Some(width));
                set_side(&mut self.border_style, i, Some(style));
            }
        }
    }

    /// `font: italic bold 12px/1.5 serif`: style, weight, size, family.
    fn font(&mut self, value: &str) {
        let lower = value.to_ascii_lowercase();
        let words: Vec<&str> = lower.split_ascii_whitespace().collect();
        let Some(at) = words.iter().position(|w| {
            let size = w.split('/').next().unwrap_or_default();
            font_size(size).is_some()
        }) else {
            return;
        };
        for w in words.iter().take(at) {
            self.italic = italic(w).or(self.italic);
            self.weight = weight(w).or(self.weight);
        }
        let mut sized = words.get(at).unwrap_or(&"").split('/');
        self.size = sized.next().and_then(font_size).or(self.size);
        self.line = sized.next().and_then(line_height).or(self.line);
        // The family keeps its case: names match faces.
        let rest: Vec<&str> = value.split_ascii_whitespace().skip(at + 1).collect();
        if !rest.is_empty() {
            self.family = Some(families(&rest.join(" ")));
        }
    }

    /// Sets each property `top` sets, as a later rule does.
    pub(crate) fn layer(&mut self, top: &Self) {
        layer!(self, top;
            italic, weight, align, display, unmarked, width, height, size, line,
            letter, word, indent, family, background, background_image, float,
            break_before, valign;
            margin, padding, border_width, border_style);
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn set_side<T: Copy>(sides: &mut [Option<T>; 4], i: usize, v: Option<T>) {
    if let (Some(slot), Some(v)) = (sides.get_mut(i), v) {
        *slot = Some(v);
    }
}

/// One to four values, as CSS spreads them over the sides.
fn sides<T: Copy>(out: &mut [Option<T>; 4], v: &str, read: impl Fn(&str) -> Option<T>) {
    let values: Vec<T> = v.split_ascii_whitespace().filter_map(read).collect();
    let pick = |order: [usize; 4]| order.map(|i| values.get(i).copied());
    let spread = match values.len() {
        1 => pick([0, 0, 0, 0]),
        2 => pick([0, 1, 0, 1]),
        3 => pick([0, 1, 2, 1]),
        4 => pick([0, 1, 2, 3]),
        _ => return,
    };
    *out = spread;
}

/// `items` split at `sep`, but not inside brackets or quotes.
fn split_top(text: &str, sep: char) -> Vec<&str> {
    let (mut out, mut depth, mut quote, mut start) = (Vec::new(), 0i32, None, 0);
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, c) if c == sep && depth <= 0 => {
                out.push(text.get(start..i).unwrap_or_default());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(text.get(start..).unwrap_or_default());
    out
}

fn italic(v: &str) -> Option<bool> {
    match v {
        "italic" | "oblique" => Some(true),
        "normal" => Some(false),
        _ => None,
    }
}

fn weight(v: &str) -> Option<Weight> {
    match v {
        "bold" => Some(Weight::Abs(700)),
        "normal" => Some(Weight::Abs(400)),
        "bolder" => Some(Weight::Bolder),
        "lighter" => Some(Weight::Lighter),
        _ => (v.parse::<u16>().ok())
            .filter(|w| (1..=1000).contains(w))
            .map(Weight::Abs),
    }
}

fn text_align(v: &str) -> Option<TextAlign> {
    match v.trim() {
        "center" | "-webkit-center" => Some(TextAlign::Center),
        "right" | "end" => Some(TextAlign::End),
        "left" | "start" => Some(TextAlign::Start),
        "justify" => Some(TextAlign::Justify),
        _ => None,
    }
}

fn display(v: &str) -> Display {
    match v.split_ascii_whitespace().last().unwrap_or_default() {
        "none" => Display::None,
        "block" | "list-item" | "flow-root" => Display::Block,
        "inline" | "inline-block" | "contents" => Display::Inline,
        "flex" | "inline-flex" | "-webkit-box" | "-webkit-flex" => Display::Flex,
        "grid" | "inline-grid" => Display::Grid,
        "table" | "inline-table" => Display::Table,
        _ => Display::Other,
    }
}

fn font_size(v: &str) -> Option<FontSize> {
    let px = |n: f32| Some(FontSize::Len(Len::Px(n)));
    match v {
        "xx-small" => px(9.0),
        "x-small" => px(10.0),
        "small" => px(13.0),
        "medium" => px(16.0),
        "large" => px(18.0),
        "x-large" => px(24.0),
        "xx-large" => px(32.0),
        "xxx-large" => px(48.0),
        "larger" => Some(FontSize::Larger),
        "smaller" => Some(FontSize::Smaller),
        _ => len(v)
            .filter(|l| !matches!(l, Len::Auto))
            .map(FontSize::Len),
    }
}

fn line_height(v: &str) -> Option<LineHeight> {
    if v == "normal" {
        return Some(LineHeight::Normal);
    }
    if let Ok(n) = v.parse::<f32>() {
        return (n.is_finite() && n >= 0.0).then_some(LineHeight::Factor(n.min(10.0)));
    }
    len(v)
        .filter(|l| !matches!(l, Len::Auto))
        .map(LineHeight::Len)
}

fn spacing(v: &str) -> Option<Len> {
    match v {
        "normal" => Some(Len::Px(0.0)),
        _ => len(v).filter(|l| !matches!(l, Len::Auto | Len::Percent(_))),
    }
}

fn border_width(v: &str) -> Option<Len> {
    match v {
        "thin" => Some(Len::Px(1.0)),
        "medium" => Some(Len::Px(3.0)),
        "thick" => Some(Len::Px(5.0)),
        _ => len(v).filter(|l| !matches!(l, Len::Auto | Len::Percent(_))),
    }
}

fn border_style(v: &str) -> Option<BorderStyle> {
    match v {
        "none" | "hidden" => Some(BorderStyle::None),
        "solid" | "groove" | "ridge" | "inset" | "outset" => Some(BorderStyle::Solid),
        "double" => Some(BorderStyle::Double),
        "dashed" => Some(BorderStyle::Dashed),
        "dotted" => Some(BorderStyle::Dotted),
        _ => None,
    }
}

/// No paint: `transparent`, `none` or a fully clear colour.
fn clear(v: &str) -> bool {
    matches!(v, "transparent" | "none" | "initial") || v.replace(' ', "").ends_with(",0)")
}

/// A colour among a `background` shorthand's words.
fn is_colour(w: &str) -> bool {
    const NOT: &[&str] = &[
        "none",
        "transparent",
        "repeat",
        "no-repeat",
        "repeat-x",
        "repeat-y",
        "space",
        "round",
        "center",
        "top",
        "left",
        "right",
        "bottom",
        "fixed",
        "scroll",
        "local",
        "cover",
        "contain",
        "auto",
        "border-box",
        "padding-box",
        "content-box",
    ];
    let named = w.chars().all(|c| c.is_ascii_alphabetic()) && !NOT.contains(&w);
    w.starts_with('#') || w.starts_with("rgb") || w.starts_with("hsl") || named
}

/// `"A", b, serif` as names, quotes gone, case kept.
fn families(v: &str) -> Arc<[String]> {
    split_top(v, ',')
        .iter()
        .map(|f| f.trim().trim_matches(['"', '\'']).trim().to_owned())
        .filter(|f| !f.is_empty())
        .take(8)
        .collect()
}

/// A font the book ships: its family as the book names it and files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontFace {
    pub(crate) family: String,
    /// Archive paths, in the order given.
    pub(crate) src: Vec<String>,
    pub(crate) weight: u16,
    pub(crate) italic: bool,
}

/// What a compound's attribute test asks.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AttrOp {
    Exists,
    Equals,
    /// `~=`: one of its words.
    Word,
    /// `|=`: equal, or that and a hyphen.
    Dash,
    Prefix,
    Suffix,
    Contains,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttrTest {
    name: String,
    op: AttrOp,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pseudo {
    /// `an+b` among element siblings; of its type with `typed`.
    Nth {
        a: i32,
        b: i32,
        typed: bool,
    },
    Root,
    Link,
    Not(Box<Compound>),
    /// Needs what the stream hasn't read yet, or user action.
    Never,
}

/// One compound selector: a tag, id, classes, attributes, pseudo-classes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<AttrTest>,
    pseudo: Vec<Pseudo>,
}

impl Compound {
    fn specificity(&self) -> u32 {
        let not: u32 = (self.pseudo.iter())
            .map(|p| match p {
                Pseudo::Not(c) => c.specificity(),
                _ => 1 << 8,
            })
            .sum();
        let qualifiers = u32::try_from(self.classes.len() + self.attrs.len()).unwrap_or(255);
        (u32::from(self.id.is_some()) << 16)
            + (qualifiers << 8)
            + u32::from(self.tag.is_some())
            + not
    }

    fn matches(&self, el: &El) -> bool {
        self.tag.as_ref().is_none_or(|t| *t == el.tag)
            && self.id.as_ref().is_none_or(|id| el.id.as_ref() == Some(id))
            && self.classes.iter().all(|c| el.classes.contains(c))
            && self.attrs.iter().all(|a| a.matches(el))
            && self.pseudo.iter().all(|p| match p {
                Pseudo::Nth { a, b, typed } => {
                    let i = if *typed { el.type_index } else { el.index };
                    nth(*a, *b, i32::try_from(i).unwrap_or(i32::MAX))
                }
                Pseudo::Root => el.tag == "html",
                Pseudo::Link => el.tag == "a" && el.attr("href").is_some(),
                Pseudo::Not(c) => !c.matches(el),
                Pseudo::Never => false,
            })
    }
}

impl AttrTest {
    fn matches(&self, el: &El) -> bool {
        let Some(v) = el.attr(&self.name) else {
            return false;
        };
        let want = self.value.as_str();
        match self.op {
            AttrOp::Exists => true,
            AttrOp::Equals => v == want,
            AttrOp::Word => v.split_ascii_whitespace().any(|w| w == want),
            AttrOp::Dash => v == want || v.strip_prefix(want).is_some_and(|r| r.starts_with('-')),
            AttrOp::Prefix => !want.is_empty() && v.starts_with(want),
            AttrOp::Suffix => !want.is_empty() && v.ends_with(want),
            AttrOp::Contains => !want.is_empty() && v.contains(want),
        }
    }
}

/// Whether the `i`th sibling (from 1) is `an+b` for some n ≥ 0.
fn nth(a: i32, b: i32, i: i32) -> bool {
    match a {
        0 => i == b,
        a => {
            let d = i - b;
            d % a == 0 && d / a >= 0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    /// `+`
    Next,
    /// `~`
    Later,
}

/// Compounds from the subject leftward, each with how it joins the next.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Selector {
    parts: Vec<(Compound, Combinator)>,
}

impl Selector {
    fn specificity(&self) -> u32 {
        self.parts.iter().map(|(c, _)| c.specificity()).sum()
    }
}

/// An element as selectors see it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct El {
    pub(crate) tag: String,
    pub(crate) id: Option<String>,
    pub(crate) classes: Vec<String>,
    /// Attributes, names lowercase.
    pub(crate) attrs: Vec<(String, String)>,
    /// Its place among its parent's elements, and among those of its tag.
    pub(crate) index: usize,
    pub(crate) type_index: usize,
}

impl El {
    fn attr(&self, name: &str) -> Option<&str> {
        (self.attrs.iter())
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// One element of the path to the subject, with its earlier siblings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ctx<'a> {
    pub(crate) el: &'a El,
    /// The latest siblings before it, oldest first.
    pub(crate) before: &'a [El],
}

/// A place in the tree: on the path, or the `j`th earlier sibling of one.
#[derive(Clone, Copy)]
enum Pos {
    Path(usize),
    Sibling(usize, usize),
}

fn el_at<'a>(path: &[Ctx<'a>], pos: Pos) -> Option<&'a El> {
    match pos {
        Pos::Path(i) => path.get(i).map(|c| c.el),
        Pos::Sibling(i, j) => path.get(i).and_then(|c| c.before.get(j)),
    }
}

fn parent(pos: Pos) -> Option<Pos> {
    let (Pos::Path(i) | Pos::Sibling(i, _)) = pos;
    i.checked_sub(1).map(Pos::Path)
}

fn previous(path: &[Ctx<'_>], pos: Pos) -> Option<Pos> {
    match pos {
        Pos::Path(i) => {
            let n = path.get(i)?.before.len();
            n.checked_sub(1).map(|j| Pos::Sibling(i, j))
        }
        Pos::Sibling(i, j) => j.checked_sub(1).map(|j| Pos::Sibling(i, j)),
    }
}

fn matches_at(parts: &[(Compound, Combinator)], path: &[Ctx<'_>], pos: Pos) -> bool {
    let Some(((compound, combinator), rest)) = parts.split_first() else {
        return true;
    };
    if !el_at(path, pos).is_some_and(|el| compound.matches(el)) {
        return false;
    }
    if rest.is_empty() {
        return true;
    }
    let step: fn(&[Ctx<'_>], Pos) -> Option<Pos> = match combinator {
        Combinator::Child | Combinator::Descendant => |_, p| parent(p),
        Combinator::Next | Combinator::Later => previous,
    };
    let mut at = step(path, pos);
    while let Some(p) = at {
        if matches_at(rest, path, p) {
            return true;
        }
        if matches!(combinator, Combinator::Child | Combinator::Next) {
            return false;
        }
        at = step(path, p);
    }
    false
}

impl Selector {
    /// Whether it matches the last element of `path`.
    fn matches(&self, path: &[Ctx<'_>]) -> bool {
        let Some(last) = path.len().checked_sub(1) else {
            return false;
        };
        matches_at(&self.parts, path, Pos::Path(last))
    }
}

/// Parses one selector; pseudo-elements and junk give `None`.
fn parse_selector(s: &str) -> Option<Selector> {
    let s = s.trim();
    if s.is_empty() || s.contains('\\') {
        return None;
    }
    let mut parts: Vec<(Compound, Combinator)> = Vec::new();
    let mut joiner = Combinator::Descendant;
    let mut rest = s;
    loop {
        let (compound, after) = parse_compound(rest)?;
        parts.push((compound, joiner));
        let trimmed = after.trim_start();
        if trimmed.is_empty() {
            break;
        }
        let spaced = trimmed.len() < after.len();
        joiner = match trimmed.chars().next() {
            Some('>') => Combinator::Child,
            Some('+') => Combinator::Next,
            Some('~') => Combinator::Later,
            _ if spaced => Combinator::Descendant,
            _ => return None,
        };
        rest = match joiner {
            Combinator::Descendant => trimmed,
            _ => trimmed.get(1..)?.trim_start(),
        };
        if parts.len() >= MAX_PARTS {
            return None;
        }
    }
    // Subject first: each part keeps how it joins the one to its left.
    parts.reverse();
    Some(Selector { parts })
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_')
}

/// An identifier at the start of `s`, and what follows.
fn ident(s: &str) -> Option<(&str, &str)> {
    let end = s.find(|c: char| !is_ident(c)).unwrap_or(s.len());
    (end > 0).then(|| s.split_at(end))
}

fn parse_compound(s: &str) -> Option<(Compound, &str)> {
    let mut c = Compound::default();
    let mut rest = s;
    if let Some(after) = rest.strip_prefix('*') {
        rest = after;
    } else if let Some((tag, after)) = ident(rest) {
        // `epub|switch`: namespaces don't matter here.
        let (tag, after) = match after.strip_prefix('|').and_then(ident) {
            Some((local, after)) => (local, after),
            None => (tag, after),
        };
        c.tag = Some(tag.to_ascii_lowercase());
        rest = after;
    }
    let start = rest.len();
    loop {
        if let Some(after) = rest.strip_prefix('#') {
            let (id, after) = ident(after)?;
            c.id = Some(id.to_owned());
            rest = after;
        } else if let Some(after) = rest.strip_prefix('.') {
            let (class, after) = ident(after)?;
            c.classes.push(class.to_owned());
            rest = after;
        } else if let Some(after) = rest.strip_prefix('[') {
            let close = after.find(']')?;
            c.attrs.push(attr_test(after.get(..close)?)?);
            rest = after.get(close + 1..)?;
        } else if let Some(after) = rest.strip_prefix(':') {
            if after.starts_with(':') {
                return None;
            }
            let (name, after) = ident(after)?;
            let (args, after) = match after.strip_prefix('(') {
                Some(inner) => {
                    let close = inner.find(')')?;
                    (Some(inner.get(..close)?), inner.get(close + 1..)?)
                }
                None => (None, after),
            };
            c.pseudo.push(pseudo(&name.to_ascii_lowercase(), args)?);
            rest = after;
        } else {
            break;
        }
    }
    let empty = c.tag.is_none() && rest.len() == start && !s.starts_with('*');
    (!empty).then_some((c, rest))
}

fn attr_test(inner: &str) -> Option<AttrTest> {
    let ops = [
        ("~=", AttrOp::Word),
        ("|=", AttrOp::Dash),
        ("^=", AttrOp::Prefix),
        ("$=", AttrOp::Suffix),
        ("*=", AttrOp::Contains),
        ("=", AttrOp::Equals),
    ];
    let found = ops
        .iter()
        .filter_map(|(sym, op)| inner.find(sym).map(|at| (at, *sym, op.clone())))
        .min_by_key(|(at, _, _)| *at);
    let (name, op, value) = match found {
        Some((at, sym, op)) => {
            let value = inner.get(at + sym.len()..)?.trim();
            let value = value.trim_end_matches([' ', 'i', 'I']).trim();
            (inner.get(..at)?, op, value.trim_matches(['"', '\'']))
        }
        None => (inner, AttrOp::Exists, ""),
    };
    // `epub|type` names the attribute `epub:type`.
    let name = name.trim().replace('|', ":").to_ascii_lowercase();
    let name = name.trim_start_matches(':').to_owned();
    (!name.is_empty()).then(|| AttrTest {
        name,
        op,
        value: value.to_owned(),
    })
}

/// A pseudo-class; pseudo-elements, written with one colon too, give none.
fn pseudo(name: &str, args: Option<&str>) -> Option<Pseudo> {
    let nth_args = |typed| {
        let (a, b) = an_b(args?)?;
        Some(Pseudo::Nth { a, b, typed })
    };
    match name {
        "before" | "after" | "first-line" | "first-letter" | "selection" | "marker" => None,
        "first-child" => Some(Pseudo::Nth {
            a: 0,
            b: 1,
            typed: false,
        }),
        "first-of-type" => Some(Pseudo::Nth {
            a: 0,
            b: 1,
            typed: true,
        }),
        "nth-child" => nth_args(false),
        "nth-of-type" => nth_args(true),
        "root" => Some(Pseudo::Root),
        "link" | "any-link" => Some(Pseudo::Link),
        "not" => {
            let (inner, rest) = parse_compound(args?.trim())?;
            rest.trim().is_empty().then(|| Pseudo::Not(Box::new(inner)))
        }
        _ => Some(Pseudo::Never),
    }
}

/// `odd`, `even`, `3`, `n`, `-n+3`, `2n+1` as a and b.
fn an_b(s: &str) -> Option<(i32, i32)> {
    let s: String = s
        .split_ascii_whitespace()
        .collect::<String>()
        .to_ascii_lowercase();
    match s.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    let Some((a, b)) = s.split_once('n') else {
        return Some((0, s.parse().ok()?));
    };
    let a = match a {
        "" | "+" => 1,
        "-" => -1,
        a => a.parse().ok()?,
    };
    let b = match b {
        "" => 0,
        b => b.trim_start_matches('+').parse().ok()?,
    };
    Some((a, b))
}

struct Rule {
    selector: Selector,
    normal: Decl,
    important: Decl,
    specificity: u32,
    /// Where it last appeared among the sheet's rules.
    order: usize,
}

/// A sheet's rules, indexed by their subject's id, class or tag.
#[derive(Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
    by_id: HashMap<String, Vec<usize>>,
    by_class: HashMap<String, ByClass>,
    by_tag: HashMap<String, Vec<usize>>,
    any: Vec<usize>,
    /// A selector seen before merges into its rule.
    known: HashMap<String, usize>,
    pub(crate) faces: Vec<FontFace>,
    seen: usize,
}

/// A class's rules: any tag, and by tag.
#[derive(Default)]
struct ByClass {
    any: Vec<usize>,
    tags: HashMap<String, Vec<usize>>,
}

impl std::fmt::Debug for Stylesheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stylesheet({} rules)", self.rules.len())
    }
}

impl Stylesheet {
    /// A sheet found at `path`, against which its `url()`s resolve.
    pub(crate) fn parse(css: &str, path: &str) -> Self {
        let mut sheet = Self::default();
        let css = strip_comments(css.get(..floor(css, MAX_SHEET)).unwrap_or_default());
        sheet.read(&css, path, 0);
        sheet
    }

    fn read(&mut self, css: &str, path: &str, depth: usize) {
        let mut rest = css;
        while let Some(open) = rest.find('{') {
            // Statements like `@import x;` end before the selector.
            let prelude = rest.get(..open).unwrap_or_default();
            let prelude = prelude.rsplit(';').next().unwrap_or_default().trim();
            let body = rest.get(open + 1..).unwrap_or_default();
            let Some(close) = matching_brace(body) else {
                break;
            };
            let inner = body.get(..close).unwrap_or_default();
            let lower = prelude.to_ascii_lowercase();
            if let Some(query) = lower.strip_prefix("@media") {
                if depth < 4 && media_applies(query) {
                    self.read(inner, path, depth + 1);
                }
            } else if lower.starts_with("@supports") {
                if depth < 4 {
                    self.read(inner, path, depth + 1);
                }
            } else if lower.starts_with("@font-face") {
                self.face(inner, path);
            } else if !prelude.starts_with('@') {
                let (normal, important) = Decl::parse_both(inner);
                if !(normal.is_empty() && important.is_empty()) {
                    for selector in split_top(prelude, ',') {
                        self.add(selector, &normal, &important);
                    }
                }
            }
            rest = body.get(close + 1..).unwrap_or_default();
        }
    }

    fn face(&mut self, body: &str, path: &str) {
        if self.faces.len() >= MAX_FACES {
            return;
        }
        let mut face = FontFace {
            family: String::new(),
            src: Vec::new(),
            weight: 400,
            italic: false,
        };
        for item in split_top(body, ';') {
            let Some((name, value)) = item.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "font-family" => {
                    face.family = families(value).first().cloned().unwrap_or_default();
                }
                "src" => face.src = urls(value, path),
                "font-weight" => {
                    if let Some(Weight::Abs(w)) = weight(&value.to_ascii_lowercase()) {
                        face.weight = w;
                    }
                }
                "font-style" => face.italic = italic(&value.to_ascii_lowercase()) == Some(true),
                _ => {}
            }
        }
        if !face.family.is_empty() && !face.src.is_empty() {
            self.faces.push(face);
        }
    }

    /// A repeated selector merges into its rule, later values winning.
    fn add(&mut self, text: &str, normal: &Decl, important: &Decl) {
        let key = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
        let order = self.seen;
        self.seen += 1;
        if let Some(rule) = self.known.get(&key).and_then(|&at| self.rules.get_mut(at)) {
            rule.normal.layer(normal);
            rule.important.layer(important);
            rule.order = order;
            return;
        }
        let Some(selector) = parse_selector(&key) else {
            return;
        };
        if self.rules.len() >= MAX_RULES {
            return;
        }
        let at = self.rules.len();
        let subject = selector
            .parts
            .first()
            .map(|(c, _)| c.clone())
            .unwrap_or_default();
        if let Some(id) = &subject.id {
            self.by_id.entry(id.clone()).or_default().push(at);
        } else if let Some(class) = subject.classes.first() {
            let by = self.by_class.entry(class.clone()).or_default();
            match &subject.tag {
                Some(tag) => by.tags.entry(tag.clone()).or_default().push(at),
                None => by.any.push(at),
            }
        } else if let Some(tag) = &subject.tag {
            self.by_tag.entry(tag.clone()).or_default().push(at);
        } else {
            self.any.push(at);
        }
        self.known.insert(key, at);
        self.rules.push(Rule {
            specificity: selector.specificity(),
            selector,
            normal: normal.clone(),
            important: important.clone(),
            order,
        });
    }

    /// Calls `found(rule)` for each rule matching the path's last element.
    fn matching<'s>(&'s self, path: &[Ctx<'_>], found: &mut impl FnMut(&'s Rule)) {
        let Some(el) = path.last().map(|c| c.el) else {
            return;
        };
        let by_id = el.id.as_ref().and_then(|id| self.by_id.get(id));
        let by_class = (el.classes.iter())
            .filter_map(|c| self.by_class.get(c))
            .flat_map(|b| {
                b.any
                    .iter()
                    .chain(b.tags.get(&el.tag).into_iter().flatten())
            });
        let candidates = (by_id.into_iter().flatten())
            .chain(by_class)
            .chain(self.by_tag.get(&el.tag).into_iter().flatten())
            .chain(&self.any);
        for &at in candidates {
            if let Some(rule) = self.rules.get(at)
                && rule.selector.matches(path)
            {
                found(rule);
            }
        }
    }
}

/// The resolved archive paths of a `src` list's `url()`s.
fn urls(value: &str, path: &str) -> Vec<String> {
    split_top(value, ',')
        .iter()
        .filter_map(|item| {
            let start = item.find("url(")? + 4;
            let inner = item.get(start..)?;
            let inner = inner.get(..inner.find(')')?)?;
            href::resolve(path, inner.trim().trim_matches(['"', '\'']))
        })
        .take(4)
        .collect()
}

/// Screen and all apply; print and other devices don't.
fn media_applies(query: &str) -> bool {
    split_top(query, ',').iter().any(|q| {
        let words: Vec<&str> = q.split_ascii_whitespace().collect();
        let (negated, words) = match words.split_first() {
            Some((&"not", rest)) => (true, rest),
            Some((&"only", rest)) => (false, rest),
            _ => (false, words.as_slice()),
        };
        let kind = words.first().copied().unwrap_or("all");
        let typed = match kind {
            k if k.starts_with('(') => true,
            "all" | "screen" => true,
            _ => false,
        };
        typed != negated
    })
}

/// The browser's own defaults, under every book sheet.
fn user_agent() -> &'static Stylesheet {
    static SHEET: OnceLock<Stylesheet> = OnceLock::new();
    SHEET.get_or_init(|| {
        Stylesheet::parse(
            "html, body, div, p, section, article, header, footer, aside, nav, main, \
             figure, figcaption, blockquote, ul, ol, dl, dt, dd, li, h1, h2, h3, h4, h5, h6, \
             pre, address, center, hr, caption { display: block }\n\
             table { display: table }\n\
             head, script, style, title, template, noscript { display: none }\n\
             p, blockquote, figure, dl, ul, ol, pre { margin: 1em 0 }\n\
             h1 { font-size: 2em; margin: 0.67em 0; font-weight: bold }\n\
             h2 { font-size: 1.5em; margin: 0.83em 0; font-weight: bold }\n\
             h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold }\n\
             h4 { margin: 1.33em 0; font-weight: bold }\n\
             h5 { font-size: 0.83em; margin: 1.67em 0; font-weight: bold }\n\
             h6 { font-size: 0.67em; margin: 2.33em 0; font-weight: bold }\n\
             b, strong, th { font-weight: bold }\n\
             i, em, cite, var, dfn, address { font-style: italic }\n\
             small, sub, sup { font-size: smaller }\n\
             big { font-size: larger }\n\
             center { text-align: center }\n\
             hr { margin: 0.5em 0 }\n\
             td, th { padding: 1px }",
            "",
        )
    })
}

/// One element's cascade: user agent, `hint`, `sheets`, `inline`.
/// `!important` declarations then win, inline ones last.
pub(crate) fn declared(
    sheets: &[Arc<Stylesheet>],
    path: &[Ctx<'_>],
    hint: &Decl,
    inline: Option<&str>,
) -> Decl {
    let mut d = Decl::default();
    user_agent().matching(path, &mut |r| d.layer(&r.normal));
    d.layer(hint);
    let mut found: Vec<(u32, usize, usize, &Rule)> = Vec::new();
    for (n, sheet) in sheets.iter().enumerate() {
        sheet.matching(path, &mut |r| found.push((r.specificity, n, r.order, r)));
    }
    found.sort_by_key(|&(spec, sheet, order, _)| (spec, sheet, order));
    for (.., r) in &found {
        d.layer(&r.normal);
    }
    let (inline, inline_important) = inline.map(Decl::parse_both).unwrap_or_default();
    d.layer(&inline);
    for (.., r) in &found {
        d.layer(&r.important);
    }
    d.layer(&inline_important);
    d
}

/// A line's height as it inherits: a factor, or computed px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Line {
    Normal,
    Factor(f32),
    Px(f32),
}

/// An element's computed style, in CSS px; sides go top, right, bottom, left.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Computed {
    pub(crate) size: f32,
    pub(crate) line: Line,
    pub(crate) weight: u16,
    pub(crate) italic: bool,
    pub(crate) family: Option<Arc<[String]>>,
    pub(crate) letter: f32,
    pub(crate) word: f32,
    /// Px, or a percentage of the column.
    pub(crate) indent: Len,
    pub(crate) align: TextAlign,
    pub(crate) unmarked: bool,
    // Not inherited.
    pub(crate) display: Display,
    /// Px, percentages of the column, or auto.
    pub(crate) margin: [Len; 4],
    pub(crate) padding: [f32; 4],
    /// Zero where no border is drawn.
    pub(crate) border: [f32; 4],
    pub(crate) border_style: [BorderStyle; 4],
    pub(crate) width: Len,
    pub(crate) height: Len,
    pub(crate) background: bool,
    pub(crate) float: bool,
    pub(crate) break_before: bool,
    /// A cell's `vertical-align`, if set.
    pub(crate) valign: Option<VAlign>,
}

impl Default for Computed {
    fn default() -> Self {
        Self {
            size: ROOT_PX,
            line: Line::Normal,
            weight: 400,
            italic: false,
            family: None,
            letter: 0.0,
            word: 0.0,
            indent: Len::Px(0.0),
            align: TextAlign::Start,
            unmarked: false,
            display: Display::Block,
            margin: [Len::Px(0.0); 4],
            padding: [0.0; 4],
            border: [0.0; 4],
            border_style: [BorderStyle::None; 4],
            width: Len::Auto,
            height: Len::Auto,
            background: false,
            float: false,
            break_before: false,
            valign: None,
        }
    }
}

impl Computed {
    /// A child's style: what `d` sets, the rest inherited or initial.
    pub(crate) fn child(&self, d: &Decl) -> Self {
        let size = match d.size {
            None => self.size,
            Some(FontSize::Larger) => self.size * 1.2,
            Some(FontSize::Smaller) => self.size / 1.2,
            Some(FontSize::Len(l)) => l.px(self.size, self.size).unwrap_or(self.size),
        }
        .clamp(1.0, 1000.0);
        let line = match d.line {
            None => self.line,
            Some(LineHeight::Normal) => Line::Normal,
            Some(LineHeight::Factor(f)) => Line::Factor(f),
            Some(LineHeight::Len(l)) => Line::Px(l.px(size, size).unwrap_or(0.0).max(0.0)),
        };
        let weight = match d.weight {
            None => self.weight,
            Some(Weight::Abs(w)) => w,
            Some(Weight::Bolder) => match self.weight {
                w if w < 350 => 400,
                w if w < 550 => 700,
                _ => 900,
            },
            Some(Weight::Lighter) => match self.weight {
                w if w < 550 => 100,
                w if w < 750 => 400,
                _ => 700,
            },
        };
        let px = |l: Option<Len>| l.and_then(|l| l.px(size, BASE_WIDTH)).unwrap_or(0.0);
        let border_style = d.border_style.map(Option::unwrap_or_default);
        let mut border = [0.0; 4];
        for ((b, w), s) in border.iter_mut().zip(d.border_width).zip(border_style) {
            if s != BorderStyle::None {
                *b = w.map_or(3.0, |w| px(Some(w))).max(0.0);
            }
        }
        Self {
            size,
            line,
            weight,
            italic: d.italic.unwrap_or(self.italic),
            family: d.family.clone().or_else(|| self.family.clone()),
            letter: d.letter.map_or(self.letter, |l| px(Some(l))),
            word: d.word.map_or(self.word, |l| px(Some(l))),
            indent: d.indent.map_or(self.indent, |l| l.computed(size)),
            align: d.align.unwrap_or(self.align),
            unmarked: d.unmarked.unwrap_or(self.unmarked),
            display: d.display.unwrap_or(Display::Inline),
            margin: d
                .margin
                .map(|m| m.map_or(Len::Px(0.0), |m| m.computed(size))),
            padding: d.padding.map(|p| px(p).max(0.0)),
            border,
            border_style,
            width: d.width.map_or(Len::Auto, |l| l.computed(size)),
            height: d.height.map_or(Len::Auto, |l| l.computed(size)),
            background: d.background.unwrap_or(false) || d.background_image.unwrap_or(false),
            float: d.float.unwrap_or(false),
            break_before: d.break_before.unwrap_or(false),
            valign: d.valign,
        }
    }

    /// One line's height in px.
    pub(crate) fn line_px(&self) -> f32 {
        match self.line {
            Line::Normal => 1.2 * self.size,
            Line::Factor(f) => f * self.size,
            Line::Px(p) => p,
        }
    }

    /// A side's margin in px; auto and percentages of the column, near enough.
    pub(crate) fn margin_px(&self, side: usize) -> f32 {
        let m = self.margin.get(side).copied().unwrap_or(Len::Px(0.0));
        m.px(self.size, BASE_WIDTH).unwrap_or(0.0)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn el(tag: &str, classes: &[&str]) -> El {
        El {
            tag: tag.to_owned(),
            classes: classes.iter().map(|c| (*c).to_owned()).collect(),
            index: 1,
            type_index: 1,
            ..El::default()
        }
    }

    /// The cascade for the last of `chain`, each a child of the one before.
    fn cascade(css: &str, chain: &[El]) -> Decl {
        let sheet = Arc::new(Stylesheet::parse(css, "/s.css"));
        let path: Vec<Ctx<'_>> = chain.iter().map(|el| Ctx { el, before: &[] }).collect();
        declared(&[sheet], &path, &Decl::default(), None)
    }

    #[test]
    fn declarations_read_the_supported_properties() {
        let d = Decl::parse("font-style: italic; FONT-WEIGHT: 700 !important; text-align:center");
        assert_eq!(d.italic, Some(true));
        assert_eq!(d.weight, Some(Weight::Abs(700)));
        assert_eq!(d.align, Some(TextAlign::Center));
        assert_eq!(Decl::parse("display: none").display, Some(Display::None));
        assert_eq!(
            Decl::parse("font: italic bold 1em serif").italic,
            Some(true)
        );
        let d = Decl::parse("margin: 1em 2px; border: #fff 1px double; letter-spacing: 0.2em");
        let (em, px) = (Some(Len::Em(1.0)), Some(Len::Px(2.0)));
        assert_eq!(d.margin, [em, px, em, px]);
        assert_eq!(d.border_style, [Some(BorderStyle::Double); 4]);
        assert_eq!(d.border_width, [Some(Len::Px(1.0)); 4]);
        assert_eq!(d.letter, Some(Len::Em(0.2)));
        let d = Decl::parse("border-top: solid 1px #FFF; font-family: \"Title A\", serif");
        assert_eq!(d.border_style[0], Some(BorderStyle::Solid));
        assert_eq!(d.border_style[1], None);
        let family: Vec<String> = d.family.iter().flat_map(|f| f.iter().cloned()).collect();
        assert_eq!(family, ["Title A", "serif"]);
        assert!(Decl::parse("color: red").is_empty());
    }

    #[test]
    fn lengths_compute_from_every_unit() {
        let px = |v: &str| len(v).and_then(|l| l.px(10.0, 200.0));
        assert_eq!(px("12pt"), Some(16.0));
        assert_eq!(px("1.5em"), Some(15.0));
        assert_eq!(px("2rem"), Some(32.0));
        assert_eq!(px("50%"), Some(100.0));
        assert_eq!(px("1in"), Some(96.0));
        assert_eq!(px("0"), Some(0.0));
        assert_eq!(px("-0.5em"), Some(-5.0));
        assert_eq!(px("3"), None);
        assert_eq!(len("auto"), Some(Len::Auto));
    }

    #[test]
    fn selectors_combine_and_specificity_orders_them() {
        let css = "p { text-align: right } div p { text-align: center }\n\
                   div > p.x { text-align: left } #me { font-style: italic }\n\
                   p:first-child { font-weight: bold } a:hover { font-weight: 900 }\n\
                   [epub|type~=\"note\"] { display: none } p::first-letter { font-size: 3em }";
        let div = el("div", &[]);
        let align = |chain: &[El]| cascade(css, chain).align;
        assert_eq!(align(&[el("p", &[])]), Some(TextAlign::End));
        assert_eq!(
            align(&[div.clone(), el("span", &[]), el("p", &[])]),
            Some(TextAlign::Center)
        );
        assert_eq!(
            align(&[div.clone(), el("p", &["x"])]),
            Some(TextAlign::Start)
        );
        assert_eq!(
            align(&[el("section", &[]), el("p", &["x"])]),
            Some(TextAlign::End)
        );
        let mut me = el("p", &[]);
        me.id = Some("me".to_owned());
        me.index = 2;
        let d = cascade(css, &[me]);
        assert_eq!((d.italic, d.weight), (Some(true), None));
        assert_eq!(cascade(css, &[el("p", &[])]).weight, Some(Weight::Abs(700)));
        assert_eq!(cascade(css, &[el("a", &[])]).weight, None);
        let mut note = el("aside", &[]);
        note.attrs
            .push(("epub:type".to_owned(), "foot note".to_owned()));
        assert_eq!(cascade(css, &[note]).display, Some(Display::None));
        assert_eq!(cascade(css, &[el("p", &[])]).size, None);
    }

    #[test]
    fn siblings_match_by_what_came_before() {
        let sheet = Arc::new(Stylesheet::parse(
            "h1 + p { font-style: italic } h1 ~ ul { font-weight: bold }",
            "/s.css",
        ));
        let (body, h1, hr) = (el("body", &[]), el("h1", &[]), el("hr", &[]));
        let p = el("p", &[]);
        let decl = |before: &[El], subject: &El| {
            let path = [
                Ctx {
                    el: &body,
                    before: &[],
                },
                Ctx {
                    el: subject,
                    before,
                },
            ];
            declared(std::slice::from_ref(&sheet), &path, &Decl::default(), None)
        };
        assert_eq!(decl(std::slice::from_ref(&h1), &p).italic, Some(true));
        assert_eq!(decl(&[h1.clone(), hr.clone()], &p).italic, None);
        assert_eq!(
            decl(&[h1, hr], &el("ul", &[])).weight,
            Some(Weight::Abs(700))
        );
    }

    #[test]
    fn important_and_inline_styles_win_in_order() {
        let sheet = Arc::new(Stylesheet::parse(
            ".a { text-align: center !important } p.a { text-align: right }",
            "/s.css",
        ));
        let p = el("p", &["a"]);
        let path = [Ctx {
            el: &p,
            before: &[],
        }];
        let d = declared(
            std::slice::from_ref(&sheet),
            &path,
            &Decl::default(),
            Some("text-align: left"),
        );
        assert_eq!(d.align, Some(TextAlign::Center));
        let d = declared(
            &[sheet],
            &path,
            &Decl::default(),
            Some("text-align: left !important"),
        );
        assert_eq!(d.align, Some(TextAlign::Start));
    }

    #[test]
    fn media_and_faces_are_read_and_print_is_not() {
        let sheet = Stylesheet::parse(
            "@media print { p { font-style: italic } }\n\
             @media screen and (min-width: 1px) { p { font-weight: bold } }\n\
             @font-face { font-family: \"title\"; src: url(../Fonts/title.ttf) format(\"truetype\") }\n\
             @font-face { font-family: broken }",
            "/OEBPS/Styles/main.css",
        );
        let p = el("p", &[]);
        let path = [Ctx {
            el: &p,
            before: &[],
        }];
        let d = declared(&[Arc::new(sheet)], &path, &Decl::default(), None);
        assert_eq!((d.italic, d.weight), (None, Some(Weight::Abs(700))));
        let sheet = Stylesheet::parse(
            "@font-face { font-family: \"title\"; src: url(../Fonts/title.ttf) }",
            "/OEBPS/Styles/main.css",
        );
        let face = sheet.faces.first().cloned();
        assert_eq!(face.as_ref().map(|f| f.family.as_str()), Some("title"));
        assert_eq!(
            face.map(|f| f.src),
            Some(vec!["/OEBPS/Fonts/title.ttf".to_owned()])
        );
    }

    #[test]
    fn computed_styles_inherit_and_resolve_lengths() {
        let root = Computed::default();
        let div = root.child(&Decl::parse("font-size: 0.9em; line-height: 130%"));
        assert!((div.size - 14.4).abs() < 0.01);
        assert!((div.line_px() - 14.4 * 1.3).abs() < 0.01);
        let p = div.child(&Decl::parse(
            "font-size: 0.5em; margin: 1em 0 -0.5em; border-top: 1px solid",
        ));
        assert!((p.size - 7.2).abs() < 0.01);
        // Percent line heights inherit as computed lengths.
        assert_eq!(p.line, div.line);
        assert!((p.margin_px(0) - 7.2).abs() < 0.01);
        assert!((p.margin_px(2) + 3.6).abs() < 0.01);
        assert_eq!(p.border, [1.0, 0.0, 0.0, 0.0]);
        let lh = p.child(&Decl::parse("line-height: 1em; font-weight: bolder"));
        assert_eq!((lh.line, lh.weight), (Line::Px(p.size), 700));
        assert_eq!(
            p.child(&Decl::parse("line-height: 1.5")).line_px(),
            1.5 * p.size
        );
    }

    #[test]
    fn junk_and_unclosed_rules_end_quietly() {
        let sheet = Stylesheet::parse("p { font-style: italic; } q { font-weight", "/s.css");
        assert_eq!(sheet.rules.len(), 1);
        let _ = Stylesheet::parse("}}}{{{ ; @ { /* ", "/s.css");
        let _ = Stylesheet::parse("a[ { x: y } :not( { } ::: {} p + { }", "/s.css");
    }

    #[test]
    fn repeated_selectors_merge_so_each_element_meets_few_rules() {
        let sheet = Stylesheet::parse(
            "p { font-style: italic; text-align: center } .x { font-weight: bold }\n\
             p { text-align: right }",
            "/s.css",
        );
        assert_eq!(sheet.rules.len(), 2);
        let p = el("p", &["x"]);
        let path = [Ctx {
            el: &p,
            before: &[],
        }];
        let mut found = 0;
        sheet.matching(&path, &mut |_| found += 1);
        assert_eq!(found, 2);
        // Thousands of rules, but few an element can meet.
        let mut css = "p { font-style: italic }".repeat(5000);
        css.extend((0..5000).map(|i| format!("t{i}.c {{ font-weight: bold }}")));
        let sheet = Stylesheet::parse(&css, "/s.css");
        let p = el("p", &["c"]);
        let path = [Ctx {
            el: &p,
            before: &[],
        }];
        let mut found = 0;
        sheet.matching(&path, &mut |_| found += 1);
        assert_eq!(found, 1);
    }
}
