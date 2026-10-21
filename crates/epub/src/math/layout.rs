//! Boxes for MathML elements, set as MathML Core does with the MATH table.
use ttf_parser::GlyphId;
use ttf_parser::math::Constants as K;

use super::font::{Font, Stretched};
use super::ops::{self, Class, Form, Op};
use super::variant::Variant;
use super::{MAX_NEST, Node};
use crate::css::{self, Rgb};

/// Marks one formula may draw.
pub(crate) const MAX_ITEMS: usize = 20_000;
/// Rows and columns one table may set.
const MAX_ROWS: usize = 500;
const MAX_COLUMNS: usize = 100;
/// The smallest script, as a share of the formula's size.
const MIN_SCALE: f32 = 0.3;
/// Space beside a fraction, so its bar stands clear of what is next.
const FRACTION_PAD: f32 = 0.1;
/// A subscript and superscript, either one maybe missing.
type Pair = (Option<Bx>, Option<Bx>);
/// A MATH constant's accessor.
type Kf = fn(&K<'static>) -> ttf_parser::math::MathValue<'static>;
/// What a `merror` is drawn in.
const ERROR: Rgb = [204, 0, 0];

/// Something drawn; `x, y` is its origin, y up from the baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Mark {
    Glyph {
        g: GlyphId,
        size: f32,
    },
    /// Filled, its bottom left at the origin.
    Rect {
        w: f32,
        h: f32,
    },
    /// Stroked from the origin.
    Line {
        dx: f32,
        dy: f32,
        t: f32,
    },
    /// Stroked round the origin.
    Oval {
        rx: f32,
        ry: f32,
        t: f32,
    },
    /// A stroked box, its bottom left at the origin.
    Frame {
        w: f32,
        h: f32,
        t: f32,
        r: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Item {
    pub(crate) mark: Mark,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) colour: Option<Rgb>,
}

/// A laid-out piece: its advance, ascent and descent in ems.
#[derive(Debug, Clone, Default)]
pub(crate) struct Bx {
    pub(crate) w: f32,
    pub(crate) a: f32,
    pub(crate) d: f32,
    italic: f32,
    accent_at: Option<f32>,
    pub(crate) items: Vec<Item>,
    /// The operator it embellishes, for spacing in its row.
    op: Option<Op>,
    /// Glyphs of a token, which scripts attach to by fixed shifts.
    token: bool,
}

impl Bx {
    fn put(&mut self, other: Bx, x: f32, y: f32) {
        let moved = other.items.into_iter().map(|mut i| {
            i.x += x;
            i.y += y;
            i
        });
        self.items.extend(moved);
        self.a = self.a.max(other.a + y);
        self.d = self.d.max(other.d - y);
    }

    fn shift(&mut self, dy: f32) {
        for i in &mut self.items {
            i.y += dy;
        }
        self.a += dy;
        self.d -= dy;
    }

    fn mark(&mut self, mark: Mark, (x, y): (f32, f32), colour: Option<Rgb>) {
        self.items.push(Item { mark, x, y, colour });
    }
}

/// What an element passes to those inside it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ctx {
    base: f32,
    level: i32,
    display: bool,
    cramped: bool,
    variant: Option<Variant>,
    colour: Option<Rgb>,
}

impl Ctx {
    pub(crate) fn new(display: bool) -> Self {
        Self {
            base: 1.0,
            level: 0,
            display,
            cramped: false,
            variant: None,
            colour: None,
        }
    }

    fn scripted(self, by: i32) -> Self {
        Self {
            level: self.level.saturating_add(by).clamp(-8, 16),
            display: false,
            ..self
        }
    }

    fn cramp(self) -> Self {
        Self {
            cramped: true,
            ..self
        }
    }
}

/// How a stretchy operator grows.
#[derive(Debug, Clone, Copy)]
enum Grow {
    /// Round the axis, past this ascent and descent.
    Up(f32, f32),
    Across(f32),
}

/// Lays out one formula; `full` once it has too many marks.
pub(crate) struct Layout<'f> {
    f: &'f Font,
    items: usize,
    pub(crate) full: bool,
}

impl<'f> Layout<'f> {
    pub(crate) fn new(f: &'f Font) -> Self {
        Self {
            f,
            items: 0,
            full: false,
        }
    }

    fn size(&self, c: Ctx) -> f32 {
        let scale = match c.level {
            ..=0 => (1.0 / 0.71f32).powi(-c.level).min(2.0),
            1 => self.f.share(K::script_percent_scale_down, 0.7),
            2 => self.f.share(K::script_script_percent_scale_down, 0.55),
            n => self.f.share(K::script_script_percent_scale_down, 0.55) * 0.71f32.powi(n - 2),
        };
        (c.base * scale.max(MIN_SCALE)).clamp(0.05, 20.0)
    }

    /// A MATH constant at `c`'s size.
    fn k(&self, c: Ctx, f: Kf) -> f32 {
        self.f.k(f) * self.size(c)
    }

    pub(crate) fn lay(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        if depth > MAX_NEST || self.full {
            self.full = true;
            return Bx::default();
        }
        let c = self.restyle(n, c);
        let deeper = depth + 1;
        match n.name.as_str() {
            "mi" | "mn" | "mtext" | "ms" => self.token(n, c),
            "mo" => self.mo(n, c, None),
            "mspace" => self.space(n, c),
            "mfrac" => self.fraction(n, c, deeper),
            "msqrt" => {
                let body = self.row(&n.kids, c.cramp(), deeper);
                self.radical(body, None, c)
            }
            "mroot" => self.root(n, c, deeper),
            "msub" | "msup" | "msubsup" => self.scripts(n, c, deeper),
            "munder" | "mover" | "munderover" => self.limits(n, c, deeper),
            "mmultiscripts" => self.multiscripts(n, c, deeper),
            "mtable" => self.table(n, c, deeper),
            "menclose" => {
                let body = self.row(&n.kids, c, deeper);
                self.enclose(body, n.attr("notation").unwrap_or("longdiv"), c)
            }
            "merror" => {
                let red = Ctx {
                    colour: Some(ERROR),
                    ..c
                };
                let body = self.row(&n.kids, red, deeper);
                self.enclose(body, "box", red)
            }
            "mphantom" => {
                let mut b = self.row(&n.kids, c, deeper);
                b.items.clear();
                b
            }
            "mpadded" => self.padded(n, c, deeper),
            "mfenced" => self.fenced(n, c, deeper),
            "semantics" => match n.kids.iter().find(|k| !k.name.starts_with("annotation")) {
                Some(k) => self.lay(k, c, deeper),
                None => Bx::default(),
            },
            "maction" => {
                let pick = n
                    .attr("selection")
                    .and_then(|s| s.trim().parse::<usize>().ok());
                let at = pick.unwrap_or(1).saturating_sub(1);
                match n.kids.get(at).or(n.kids.first()) {
                    Some(k) => self.lay(k, c, deeper),
                    None => Bx::default(),
                }
            }
            "mstack" | "mlongdiv" => self.stack(n, c, deeper),
            "annotation" | "annotation-xml" | "none" | "mprescripts" | "mglyph" => Bx::default(),
            _ => self.row(&n.kids, c, deeper),
        }
    }

    /// Attributes any element may set for those inside it.
    fn restyle(&self, n: &Node, mut c: Ctx) -> Ctx {
        match n.attr("displaystyle").map(str::trim) {
            Some("true") => c.display = true,
            Some("false") => c.display = false,
            _ => {}
        }
        if let Some(v) = n.attr("scriptlevel").map(str::trim) {
            let by = |s: &str| s.parse::<i32>().ok();
            if let Some(rel) = v.strip_prefix('+').and_then(by) {
                c.level = c.level.saturating_add(rel).clamp(-8, 16);
            } else if let Some(abs) = by(v) {
                // Negative alone means lower, as `-1` is written.
                c.level = if v.starts_with('-') {
                    c.level.saturating_add(abs).clamp(-8, 16)
                } else {
                    abs.clamp(-8, 16)
                };
            }
        }
        if n.name != "mi"
            && let Some(v) = n.attr("mathvariant").and_then(Variant::parse)
        {
            c.variant = Some(v);
        }
        let colour = n.attr("mathcolor").or_else(|| n.attr("color"));
        if let Some(rgb) = colour.and_then(css::colour) {
            c.colour = rgb;
        }
        let size = n.attr("mathsize").or_else(|| n.attr("fontsize"));
        if let Some(v) = size.map(str::trim) {
            let now = self.size(c);
            let scaled = match v {
                "small" => Some(now * 0.8),
                "normal" => Some(now),
                "big" => Some(now * 1.25),
                _ => self.length(v, now, now),
            };
            if let Some(s) = scaled.filter(|s| *s > 0.0) {
                c.base = (c.base * s / now.max(0.01)).clamp(0.1, 10.0);
            }
        }
        c
    }

    /// `v` as a length in ems; `size` is the em, `whole` what 100% is.
    fn length(&self, v: &str, size: f32, whole: f32) -> Option<f32> {
        let v = v.trim();
        if let Some(em) = ops::named_space(v) {
            return Some(em * size);
        }
        let split = v
            .char_indices()
            .find(|(_, ch)| !(ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+')))
            .map_or(v.len(), |(i, _)| i);
        let (number, unit) = (v.get(..split)?, v.get(split..)?.trim());
        let x: f32 = number.parse().ok().filter(|x: &f32| x.is_finite())?;
        let ex = self.f.k(K::accent_base_height).max(0.3);
        let em = match unit {
            "em" => x * size,
            "ex" => x * ex * size,
            "px" => x / 16.0,
            "pt" => x / 12.0,
            "pc" => x,
            "in" => x * 6.0,
            "cm" => x * 6.0 / 2.54,
            "mm" => x * 0.6 / 2.54,
            "mu" => x / 18.0 * size,
            "%" => x / 100.0 * whole,
            _ => return None,
        };
        Some(em.clamp(-100.0, 100.0))
    }

    fn glyph(&mut self, g: GlyphId, size: f32, colour: Option<Rgb>) -> Bx {
        let mut b = Bx::default();
        if self.items >= MAX_ITEMS {
            self.full = true;
            return b;
        }
        self.items += 1;
        let (a, d) = self.f.extent(g);
        b.w = self.f.advance(g) * size;
        (b.a, b.d) = (a * size, d * size);
        b.italic = self.f.italic(g) * size;
        b.accent_at = self.f.accent_at(g).map(|x| x * size);
        b.mark(Mark::Glyph { g, size }, (0.0, 0.0), colour);
        b
    }

    /// Letters set in a line, each in `variant`'s alphabet.
    fn run(&mut self, text: &str, variant: Option<Variant>, size: f32, colour: Option<Rgb>) -> Bx {
        let mut b = Bx {
            token: true,
            ..Bx::default()
        };
        let mut count = 0;
        for ch in text.chars() {
            let mapped = variant.map_or(ch, |v| v.map(ch));
            let found = self.f.glyph(mapped).or_else(|| self.f.glyph(ch));
            let Some(g) = found.or_else(|| self.f.glyph('?')) else {
                b.w += 0.5 * size;
                continue;
            };
            let one = self.glyph(g, size, colour);
            let (italic, accent) = (one.italic, one.accent_at);
            let x = b.w;
            b.w += one.w;
            b.put(one, x, 0.0);
            b.italic = italic;
            b.accent_at = accent.map(|a| a + x);
            count += 1;
        }
        if count != 1 {
            b.accent_at = None;
        }
        b
    }

    fn token(&mut self, n: &Node, c: Ctx) -> Bx {
        let mut text = n.words();
        if n.name == "ms" {
            let quote = |name: &str| n.attr(name).unwrap_or("\"").to_owned();
            text = format!("{}{text}{}", quote("lquote"), quote("rquote"));
        }
        let own = n.attr("mathvariant").and_then(Variant::parse);
        let single = text.chars().count() == 1;
        let variant = match (own, c.variant) {
            (Some(v), _) => Some(v),
            (None, Some(v)) => Some(v),
            (None, None) if n.name == "mi" && single => Some(Variant::Italic),
            _ => None,
        };
        self.run(&text, variant, self.size(c), c.colour)
    }

    fn space(&self, n: &Node, c: Ctx) -> Bx {
        let size = self.size(c);
        let get = |name: &str| n.attr(name).and_then(|v| self.length(v, size, 0.0));
        Bx {
            w: get("width").unwrap_or(0.0),
            a: get("height").unwrap_or(0.0),
            d: get("depth").unwrap_or(0.0),
            ..Bx::default()
        }
    }

    /// What `mo` shows: a hyphen is a minus sign, as in print.
    fn mo_text(n: &Node) -> String {
        let text = n.words();
        match text.as_str() {
            "-" => "\u{2212}".to_owned(),
            _ => text,
        }
    }

    /// The dictionary's word on `n` in `form`, then its own attributes.
    fn op_for(&self, n: &Node, form: Form) -> (Op, bool) {
        let form = n.attr("form").and_then(Form::parse).unwrap_or(form);
        let mut op = ops::lookup(&Self::mo_text(n), form);
        let mut spaced = false;
        if let Some(v) = n.attr("lspace").and_then(|v| self.length(v, 1.0, 0.0)) {
            op.lspace = v;
            spaced = true;
        }
        if let Some(v) = n.attr("rspace").and_then(|v| self.length(v, 1.0, 0.0)) {
            op.rspace = v;
            spaced = true;
        }
        let flag = |name: &str| match n.attr(name).map(str::trim) {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        };
        op.stretchy = flag("stretchy").unwrap_or(op.stretchy);
        op.largeop = flag("largeop").unwrap_or(op.largeop);
        op.movable = flag("movablelimits").unwrap_or(op.movable);
        op.accent = flag("accent").unwrap_or(op.accent);
        (op, spaced)
    }

    fn mo(&mut self, n: &Node, c: Ctx, grow: Option<Grow>) -> Bx {
        let text = Self::mo_text(n);
        let (op, _) = self.op_for(n, Form::Infix);
        let size = self.size(c);
        let mut b = Bx::default();
        if op.class == Class::Invisible || text.is_empty() {
            b.op = Some(op);
            return b;
        }
        let mut chars = text.chars();
        let one = match (chars.next(), chars.next()) {
            (Some(ch), None) => self.f.glyph(ch),
            _ => None,
        };
        let axis = self.k(c, K::axis_height);
        b = match (one, grow) {
            (Some(g), _) if op.largeop => {
                let least = self.f.units(K::display_operator_min_height);
                let grown = c.display.then(|| self.f.stretch(g, least, true)).flatten();
                let mut b = match grown {
                    Some(s) => self.stretched(s, size, true, c.colour),
                    None => self.glyph(g, size, c.colour),
                };
                b.shift(axis - (b.a - b.d) / 2.0);
                b
            }
            (Some(g), Some(Grow::Up(a, d))) if op.stretchy => {
                let goal = 2.0 * (a - axis).max(d + axis);
                let (ga, gd) = self.f.extent(g);
                match self.f.stretch(g, goal / size, true) {
                    Some(s) if goal > (ga + gd) * size => {
                        let mut b = self.stretched(s, size, true, c.colour);
                        b.shift(axis - (b.a - b.d) / 2.0);
                        b
                    }
                    _ => self.glyph(g, size, c.colour),
                }
            }
            (Some(g), Some(Grow::Across(w))) if op.stretchy => {
                match self.f.stretch(g, w / size, false) {
                    Some(s) if w > self.f.advance(g) * size => {
                        self.stretched(s, size, false, c.colour)
                    }
                    _ => self.glyph(g, size, c.colour),
                }
            }
            _ => self.run(&text, None, size, c.colour),
        };
        b.token = false;
        b.op = Some(op);
        b
    }

    fn stretched(&mut self, s: Stretched, size: f32, up: bool, colour: Option<Rgb>) -> Bx {
        match s {
            Stretched::One(g) => self.glyph(g, size, colour),
            Stretched::Parts(parts, length) => {
                let mut b = Bx::default();
                for (g, at) in parts {
                    let one = self.glyph(g, size, colour);
                    let w = one.w;
                    match up {
                        true => {
                            b.put(one, 0.0, at * size);
                            b.w = b.w.max(w);
                        }
                        false => b.put(one, at * size, 0.0),
                    }
                }
                if !up {
                    b.w = length * size;
                }
                b
            }
        }
    }

    /// The `mo` an element embellishes, which spaces it in its row.
    fn core(n: &Node) -> Option<&Node> {
        match n.name.as_str() {
            "mo" => Some(n),
            "msub" | "msup" | "msubsup" | "munder" | "mover" | "munderover" | "mmultiscripts"
            | "mfrac" | "semantics" | "maction" => n.kids.first().and_then(Self::core),
            "mrow" | "mstyle" | "mphantom" | "mpadded" => {
                let mut solid = n.kids.iter().filter(|k| !space_like(k));
                match (solid.next(), solid.next()) {
                    (Some(only), None) => Self::core(only),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Children in a line, fences grown to them and operators spaced.
    fn row(&mut self, kids: &[Node], c: Ctx, depth: usize) -> Bx {
        let kids: Vec<&Node> = kids.iter().take(MAX_ITEMS).collect();
        if let [only] = kids.as_slice() {
            return self.lay(only, c, depth);
        }
        let grows: Vec<bool> = kids
            .iter()
            .map(|k| k.name == "mo" && self.op_for(k, Form::Infix).0.stretchy)
            .collect();
        let mut boxes: Vec<Option<Bx>> = Vec::with_capacity(kids.len());
        for (k, grows) in kids.iter().zip(grows) {
            boxes.push((!grows).then(|| self.lay(k, c, depth)));
        }
        let laid = boxes.iter().flatten();
        let (a, d) = laid.fold((0.0f32, 0.0f32), |(a, d), b| (a.max(b.a), d.max(b.d)));
        for (b, k) in boxes.iter_mut().zip(&kids) {
            if b.is_none() {
                *b = Some(self.mo(k, c, Some(Grow::Up(a, d))));
            }
        }
        let solid: Vec<usize> = (0..kids.len())
            .filter(|i| kids.get(*i).is_some_and(|k| !space_like(k)))
            .collect();
        let (first, last) = (solid.first().copied(), solid.last().copied());
        let size = self.size(c);
        let mut out = Bx::default();
        let mut x = 0.0;
        let mut before: Option<(Class, Form)> = None;
        for (i, (b, k)) in boxes.into_iter().zip(&kids).enumerate() {
            let Some(b) = b else { continue };
            let Some(core) = Self::core(k) else {
                let w = b.w;
                out.put(b, x, 0.0);
                x += w;
                before = None;
                continue;
            };
            let form = match (solid.len() > 1, Some(i)) {
                (true, f) if f == first => Form::Prefix,
                (true, l) if l == last => Form::Postfix,
                _ => Form::Infix,
            };
            let (mut op, spaced) = self.op_for(core, form);
            // A sign after an operator or an opening, as in `= −x`, is unary.
            let after_op = before.is_some_and(|(class, f)| {
                matches!(
                    class,
                    Class::Relation | Class::Binary | Class::Punct | Class::Large
                ) || (class == Class::Fence && f == Form::Prefix)
            });
            if op.class == Class::Binary && form == Form::Infix && after_op && !spaced {
                op = self.op_for(core, Form::Prefix).0;
            }
            if c.level > 0 && !spaced && matches!(op.class, Class::Relation | Class::Binary) {
                (op.lspace, op.rspace) = (0.0, 0.0);
            }
            if op.class == Class::Invisible {
                x += self.applied(&kids, i, size);
            }
            x += op.lspace * size;
            let w = b.w;
            out.put(b, x, 0.0);
            x += w + op.rspace * size;
            before = Some((op.class, form));
        }
        out.w = x.max(0.0);
        out
    }

    /// A thin space after a named function applied, as in `sin x`.
    fn applied(&self, kids: &[&Node], i: usize, size: f32) -> f32 {
        let named = |k: &Node| {
            let base = match k.name.as_str() {
                "msub" | "msup" | "msubsup" | "munder" | "mover" => k.kids.first(),
                _ => Some(k),
            };
            base.is_some_and(|b| b.name == "mi" && b.words().chars().count() > 1)
        };
        let prev = i.checked_sub(1).and_then(|p| kids.get(p));
        let apply = kids
            .get(i)
            .is_some_and(|k| k.name == "mo" && k.words() == "\u{2061}");
        let fenced = kids
            .get(i + 1)
            .is_some_and(|k| k.name == "mo" && is_opening(&k.words()));
        match (prev, apply) {
            (Some(p), true) if named(p) && !fenced => 3.0 / 18.0 * size,
            _ => 0.0,
        }
    }

    fn fraction(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        let (Some(top), Some(bottom)) = (n.kids.first(), n.kids.get(1)) else {
            return self.row(&n.kids, c, depth);
        };
        let inner = match c.display {
            true => Ctx {
                display: false,
                ..c
            },
            false => c.scripted(1),
        };
        let num = self.lay(top, inner, depth);
        let den = self.lay(bottom, inner.cramp(), depth);
        let size = self.size(c);
        let rule = self.k(c, K::fraction_rule_thickness);
        let t = match n.attr("linethickness").map(str::trim) {
            Some("thin") => rule * 0.5,
            Some("medium") => rule,
            Some("thick") => rule * 2.0,
            Some(v) => match v.parse::<f32>() {
                Ok(x) if x.is_finite() => x * rule,
                _ => self.length(v, size, rule).unwrap_or(rule),
            },
            None => rule,
        }
        .clamp(0.0, size);
        let axis = self.k(c, K::axis_height);
        if n.attr("bevelled").map(str::trim) == Some("true") {
            return self.bevelled(num, den, c);
        }
        let (mut u, mut v);
        if t > 0.0 {
            let pick = |d: Kf, s: Kf| if c.display { d } else { s };
            u = self.k(
                c,
                pick(
                    K::fraction_numerator_display_style_shift_up,
                    K::fraction_numerator_shift_up,
                ),
            );
            v = self.k(
                c,
                pick(
                    K::fraction_denominator_display_style_shift_down,
                    K::fraction_denominator_shift_down,
                ),
            );
            let gap_num = self.k(
                c,
                pick(
                    K::fraction_num_display_style_gap_min,
                    K::fraction_numerator_gap_min,
                ),
            );
            let gap_den = self.k(
                c,
                pick(
                    K::fraction_denom_display_style_gap_min,
                    K::fraction_denominator_gap_min,
                ),
            );
            u = u.max(axis + t / 2.0 + gap_num + num.d);
            v = v.max(gap_den + t / 2.0 - axis + den.a);
        } else {
            let pick = |d: Kf, s: Kf| if c.display { d } else { s };
            u = self.k(
                c,
                pick(K::stack_top_display_style_shift_up, K::stack_top_shift_up),
            );
            v = self.k(
                c,
                pick(
                    K::stack_bottom_display_style_shift_down,
                    K::stack_bottom_shift_down,
                ),
            );
            let least = self.k(c, pick(K::stack_display_style_gap_min, K::stack_gap_min));
            let gap = (u - num.d) - (den.a - v);
            if gap < least {
                u += (least - gap) / 2.0;
                v += (least - gap) / 2.0;
            }
        }
        let pad = FRACTION_PAD * size;
        let w = num.w.max(den.w) + 2.0 * pad;
        let at = |name: &str, inner: f32| match n.attr(name).map(str::trim) {
            Some("left") => pad,
            Some("right") => w - pad - inner,
            _ => (w - inner) / 2.0,
        };
        let (nx, dx) = (at("numalign", num.w), at("denomalign", den.w));
        let mut out = Bx::default();
        out.put(num, nx, u);
        out.put(den, dx, -v);
        if t > 0.0 {
            let mark = Mark::Rect { w: w - pad, h: t };
            out.mark(mark, (pad / 2.0, axis - t / 2.0), c.colour);
            out.a = out.a.max(axis + t / 2.0);
            out.d = out.d.max(t / 2.0 - axis);
        }
        out.w = w;
        out
    }

    /// `a/b` set on a slant, numerator up and denominator down.
    fn bevelled(&mut self, num: Bx, den: Bx, c: Ctx) -> Bx {
        let size = self.size(c);
        let mut slash = match self.f.glyph('/') {
            Some(g) => self.glyph(g, size, c.colour),
            None => Bx::default(),
        };
        slash.token = false;
        let mut out = Bx::default();
        let (lift, drop) = (0.3 * size, 0.3 * size);
        let nw = num.w;
        out.put(num, 0.0, lift);
        let sw = slash.w;
        out.put(slash, nw, 0.0);
        let dw = den.w;
        out.put(den, nw + sw, -drop);
        out.w = nw + sw + dw;
        out
    }

    fn root(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        let (Some(base), Some(index)) = (n.kids.first(), n.kids.get(1)) else {
            let body = self.row(&n.kids, c.cramp(), depth);
            return self.radical(body, None, c);
        };
        let body = self.lay(base, c.cramp(), depth);
        let index = self.lay(index, c.scripted(2), depth);
        self.radical(body, Some(index), c)
    }

    fn radical(&mut self, body: Bx, index: Option<Bx>, c: Ctx) -> Bx {
        let size = self.size(c);
        let t = self.k(c, K::radical_rule_thickness);
        let mut gap = match c.display {
            true => self.k(c, K::radical_display_style_vertical_gap),
            false => self.k(c, K::radical_vertical_gap),
        };
        let extra = self.k(c, K::radical_extra_ascender);
        let goal = body.a + body.d + gap + t;
        let surd = self.f.glyph('√');
        let grown = surd.and_then(|g| self.f.stretch(g, goal / size, true));
        let surd = match (grown, surd) {
            (Some(s), _) => self.stretched(s, size, true, c.colour),
            (None, Some(g)) => self.glyph(g, size, c.colour),
            (None, None) => Bx::default(),
        };
        let tall = surd.a + surd.d;
        if tall > goal {
            gap += (tall - goal) / 2.0;
        }
        let top = body.a + gap + t;
        let dy = top - surd.a;
        let mut out = Bx::default();
        let mut x0 = 0.0;
        if let Some(index) = index {
            let before = self.k(c, K::radical_kern_before_degree);
            let after = self.k(c, K::radical_kern_after_degree);
            let raise = self.f.share(K::radical_degree_bottom_raise_percent, 0.6) * tall;
            let y = dy - surd.d + raise + index.d;
            let iw = index.w;
            out.put(index, before.max(0.0), y);
            x0 = (before.max(0.0) + iw + after).max(0.0);
        }
        let sw = surd.w;
        out.put(surd, x0, dy);
        let bw = body.w;
        out.mark(Mark::Rect { w: bw, h: t }, (x0 + sw, top - t), c.colour);
        out.put(body, x0 + sw, 0.0);
        out.a = out.a.max(top + extra);
        out.w = x0 + sw + bw;
        out
    }

    fn scripts(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        let Some(base) = n.kids.first() else {
            return Bx::default();
        };
        let base = self.lay(base, c, depth);
        let script = |l: &mut Self, i: usize, cramp: bool| {
            let ctx = if cramp {
                c.scripted(1).cramp()
            } else {
                c.scripted(1)
            };
            n.kids.get(i).map(|k| l.lay(k, ctx, depth))
        };
        let (sub, sup) = match n.name.as_str() {
            "msub" => (script(self, 1, true), None),
            "msup" => (None, script(self, 1, false)),
            _ => (script(self, 1, true), script(self, 2, false)),
        };
        self.attach(base, sub, sup, c)
    }

    /// How far down a subscript and up a superscript go from the base.
    fn shifts(
        &self,
        base: &Bx,
        sub: Option<(f32, f32)>,
        sup: Option<(f32, f32)>,
        c: Ctx,
    ) -> (f32, f32) {
        let k = |f: Kf| self.k(c, f);
        let token = base.token;
        let mut v = 0.0f32;
        if let Some((a, _)) = sub {
            let drop = if token {
                0.0
            } else {
                base.d + k(K::subscript_baseline_drop_min)
            };
            v = k(K::subscript_shift_down)
                .max(drop)
                .max(a - k(K::subscript_top_max));
        }
        let mut u = 0.0f32;
        if let Some((_, d)) = sup {
            let least = match c.cramped {
                true => k(K::superscript_shift_up_cramped),
                false => k(K::superscript_shift_up),
            };
            let drop = if token {
                0.0
            } else {
                base.a - k(K::superscript_baseline_drop_max)
            };
            u = least.max(drop).max(d + k(K::superscript_bottom_min));
        }
        if let (Some((sub_a, _)), Some((_, sup_d))) = (sub, sup) {
            let gap = (u - sup_d) - (sub_a - v);
            let least = k(K::sub_superscript_gap_min);
            if gap < least {
                v += least - gap;
                let lift = k(K::superscript_bottom_max_with_subscript) - (u - sup_d);
                if lift > 0.0 {
                    u += lift;
                    v -= lift;
                }
            }
        }
        (v, u)
    }

    fn attach(&mut self, base: Bx, sub: Option<Bx>, sup: Option<Bx>, c: Ctx) -> Bx {
        let dims = |b: &Option<Bx>| b.as_ref().map(|b| (b.a, b.d));
        let (v, u) = self.shifts(&base, dims(&sub), dims(&sup), c);
        let large = base.op.is_some_and(|o| o.largeop);
        let (sub_x, sup_x) = match large {
            true => (base.w - base.italic, base.w),
            false => (base.w, base.w + base.italic),
        };
        let gap = self.k(c, K::space_after_script);
        let op = base.op;
        let mut out = Bx::default();
        let mut w = base.w;
        out.put(base, 0.0, 0.0);
        if let Some(s) = sub {
            w = w.max(sub_x + s.w);
            out.put(s, sub_x, -v);
        }
        if let Some(s) = sup {
            w = w.max(sup_x + s.w);
            out.put(s, sup_x, u);
        }
        out.w = w + gap;
        out.op = op;
        out
    }

    fn limits(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        let Some(base_node) = n.kids.first() else {
            return Bx::default();
        };
        let core_op = Self::core(base_node).map(|m| self.op_for(m, Form::Prefix).0);
        let movable = core_op.is_some_and(|o| o.movable) && !c.display;
        let (under_node, over_node) = match n.name.as_str() {
            "munder" => (n.kids.get(1), None),
            "mover" => (None, n.kids.get(1)),
            _ => (n.kids.get(1), n.kids.get(2)),
        };
        let accent_of = |name: &str, k: Option<&Node>| match n.attr(name).map(str::trim) {
            Some("true") => true,
            Some("false") => false,
            _ => k
                .and_then(Self::core)
                .is_some_and(|m| self.op_for(m, Form::Postfix).0.accent),
        };
        let (accent, under_accent) = (
            accent_of("accent", over_node),
            accent_of("accentunder", under_node),
        );
        let ctx = |accent: bool| match accent {
            true => Ctx {
                display: false,
                ..c
            },
            false => c.scripted(1),
        };
        if movable {
            let base = self.lay(base_node, c, depth);
            let under = under_node.map(|k| self.lay(k, ctx(false).cramp(), depth));
            let over = over_node.map(|k| self.lay(k, ctx(false), depth));
            return self.attach(base, under, over, c);
        }
        let across = |l: &Self, k: &Node| k.name == "mo" && l.op_for(k, Form::Infix).0.stretchy;
        let lay_fixed = |l: &mut Self, k: Option<&Node>, cx: Ctx| match k {
            Some(k) if !across(l, k) => Some(l.lay(k, cx, depth)),
            _ => None,
        };
        let mut base = lay_fixed(self, Some(base_node), c);
        let mut under = lay_fixed(self, under_node, ctx(under_accent).cramp());
        let mut over = lay_fixed(self, over_node, ctx(accent));
        let widest = [&base, &under, &over]
            .iter()
            .filter_map(|b| b.as_ref().map(|b| b.w))
            .fold(0.0, f32::max);
        let grown = |l: &mut Self, k: Option<&Node>, cx: Ctx| {
            k.map(|k| l.mo(k, cx, Some(Grow::Across(widest))))
        };
        if base.is_none() {
            base = grown(self, Some(base_node), c);
        }
        if under.is_none() {
            under = grown(self, under_node, ctx(under_accent).cramp());
        }
        if over.is_none() {
            over = grown(self, over_node, ctx(accent));
        }
        let base = base.unwrap_or_default();
        let large = base.op.is_some_and(|o| o.largeop);
        let k = |f: Kf| self.k(c, f);
        let mut parts: Vec<(Bx, f32, f32)> = Vec::with_capacity(3);
        let (bw, ba, bd, italic) = (base.w, base.a, base.d, base.italic);
        let base_at = base.accent_at;
        let op = base.op;
        if let Some(o) = over {
            let (x, y) = if accent {
                let lift = (ba - k(K::accent_base_height)).max(0.0);
                let x = match (base_at, o.accent_at) {
                    (Some(b), Some(a)) => b - a,
                    _ => (bw - o.w) / 2.0,
                };
                (x, lift)
            } else {
                let mut y = ba + k(K::upper_limit_gap_min) + o.d;
                if large {
                    y = y.max(ba + k(K::upper_limit_baseline_rise_min));
                }
                ((bw - o.w) / 2.0 + if large { italic / 2.0 } else { 0.0 }, y)
            };
            parts.push((o, x, y));
        }
        if let Some(u) = under {
            let y = if under_accent {
                -(bd + u.a)
            } else {
                let mut drop = bd + k(K::lower_limit_gap_min) + u.a;
                if large {
                    drop = drop.max(k(K::lower_limit_baseline_drop_min));
                }
                -drop
            };
            let x = (bw - u.w) / 2.0 - if large { italic / 2.0 } else { 0.0 };
            parts.push((u, x, y));
        }
        let left = parts.iter().map(|(_, x, _)| *x).fold(0.0f32, f32::min);
        let mut out = Bx::default();
        out.put(base, -left, 0.0);
        let mut right = bw - left;
        for (b, x, y) in parts {
            right = right.max(x - left + b.w);
            out.put(b, x - left, y);
        }
        out.w = right;
        out.op = op;
        out
    }

    fn multiscripts(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        let Some(base) = n.kids.first() else {
            return Bx::default();
        };
        let base = self.lay(base, c, depth);
        let (mut post, mut pre) = (Vec::new(), Vec::new());
        let mut into_pre = false;
        let mut pending: Option<Option<Bx>> = None;
        for k in n.kids.iter().skip(1) {
            if k.name == "mprescripts" {
                into_pre = true;
                pending = None;
                continue;
            }
            let cramp = pending.is_none();
            let ctx = if cramp {
                c.scripted(1).cramp()
            } else {
                c.scripted(1)
            };
            let b = (k.name != "none").then(|| self.lay(k, ctx, depth));
            match pending.take() {
                None => pending = Some(b),
                Some(sub) => match into_pre {
                    true => pre.push((sub, b)),
                    false => post.push((sub, b)),
                },
            }
        }
        let all = post.iter().chain(&pre);
        let most = |pick: fn(&Pair) -> Option<&Bx>| {
            let found: Vec<&Bx> = all.clone().filter_map(pick).collect();
            (!found.is_empty()).then(|| {
                found
                    .iter()
                    .fold((0.0f32, 0.0f32), |(a, d), b| (a.max(b.a), d.max(b.d)))
            })
        };
        let sub = most(|p| p.0.as_ref());
        let sup = most(|p| p.1.as_ref());
        let (v, u) = self.shifts(&base, sub, sup, c);
        let gap = self.k(c, K::space_after_script);
        let mut out = Bx::default();
        let mut x = 0.0;
        for (sub, sup) in pre.into_iter().rev() {
            let w = sub
                .as_ref()
                .map_or(0.0, |b| b.w)
                .max(sup.as_ref().map_or(0.0, |b| b.w));
            x += gap;
            if let Some(b) = sub {
                let bw = b.w;
                out.put(b, x + w - bw, -v);
            }
            if let Some(b) = sup {
                let bw = b.w;
                out.put(b, x + w - bw, u);
            }
            x += w;
        }
        let (bw, italic, op) = (base.w, base.italic, base.op);
        out.put(base, x, 0.0);
        x += bw;
        for (i, (sub, sup)) in post.into_iter().enumerate() {
            let lean = if i == 0 { italic } else { 0.0 };
            let w = sub
                .as_ref()
                .map_or(0.0, |b| b.w)
                .max(sup.as_ref().map_or(0.0, |b| b.w + lean));
            if let Some(b) = sub {
                out.put(b, x, -v);
            }
            if let Some(b) = sup {
                out.put(b, x + lean, u);
            }
            x += w + gap;
        }
        out.w = x;
        out.op = op;
        out
    }

    fn table(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        let inner = Ctx {
            display: false,
            ..c
        };
        let size = self.size(c);
        let mut rows: Vec<Vec<(Bx, Option<&str>)>> = Vec::new();
        for r in n.kids.iter().take(MAX_ROWS) {
            let cells: Vec<&Node> = match r.name.as_str() {
                "mtr" => r.kids.iter().collect(),
                // The label is left out: it would sit at the page's edge.
                "mlabeledtr" => r.kids.iter().skip(1).collect(),
                _ => vec![r],
            };
            let row = cells
                .into_iter()
                .take(MAX_COLUMNS)
                .map(|cell| match cell.name.as_str() {
                    "mtd" => (
                        self.row(&cell.kids, self.restyle(cell, inner), depth + 1),
                        cell.attr("columnalign"),
                    ),
                    _ => (self.lay(cell, inner, depth + 1), None),
                })
                .collect();
            rows.push(row);
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return Bx::default();
        }
        let mut widths = vec![0.0f32; columns];
        for row in &rows {
            for (w, (b, _)) in widths.iter_mut().zip(row) {
                *w = w.max(b.w);
            }
        }
        let list = |name: &str| -> Vec<String> {
            n.attr(name)
                .map(|v| v.split_whitespace().map(str::to_owned).collect())
                .unwrap_or_default()
        };
        let nth = |l: &[String], i: usize| l.get(i).or(l.last()).cloned();
        let ex = self.f.k(K::accent_base_height).max(0.3) * size;
        let col_gaps = list("columnspacing");
        let row_gaps = list("rowspacing");
        let aligns = list("columnalign");
        let col_lines = list("columnlines");
        let row_lines = list("rowlines");
        let gap_x = |i: usize| {
            nth(&col_gaps, i)
                .and_then(|v| self.length(&v, size, size))
                .unwrap_or(0.8 * size)
        };
        let gap_y = |i: usize| {
            nth(&row_gaps, i)
                .and_then(|v| self.length(&v, size, size))
                .unwrap_or(ex)
        };
        let frame = matches!(n.attr("frame").map(str::trim), Some("solid" | "dashed"));
        let (fx, fy) = if frame {
            (0.4 * size, 0.5 * ex)
        } else {
            (0.0, 0.0)
        };
        let mut xs = Vec::with_capacity(columns);
        let mut x = fx;
        for (i, w) in widths.iter().enumerate() {
            xs.push(x);
            x += w + if i + 1 < columns { gap_x(i) } else { 0.0 };
        }
        let width = x + fx;
        let heights: Vec<(f32, f32)> = rows
            .iter()
            .map(|r| {
                r.iter()
                    .fold((0.0f32, 0.0f32), |(a, d), (b, _)| (a.max(b.a), d.max(b.d)))
            })
            .collect();
        let total: f32 = heights.iter().map(|(a, d)| a + d).sum::<f32>()
            + (0..heights.len().saturating_sub(1)).map(gap_y).sum::<f32>()
            + 2.0 * fy;
        let axis = self.k(c, K::axis_height);
        let top = total / 2.0 + axis;
        let t = self.k(c, K::fraction_rule_thickness).max(0.02 * size);
        let mut out = Bx::default();
        let mut y = top - fy;
        let mut row_edges = Vec::new();
        let count = rows.len();
        for (r, (row, (a, d))) in rows.into_iter().zip(heights).enumerate() {
            let base = y - a;
            for (i, (b, own)) in row.into_iter().enumerate() {
                let (Some(cx), Some(cw)) = (xs.get(i), widths.get(i)) else {
                    continue;
                };
                let align = own.map(str::to_owned).or_else(|| nth(&aligns, i));
                let dx = match align.as_deref() {
                    Some("left") => 0.0,
                    Some("right") => cw - b.w,
                    _ => (cw - b.w) / 2.0,
                };
                out.put(b, cx + dx, base);
            }
            y = base - d;
            if r + 1 < count {
                let g = gap_y(r);
                row_edges.push((r, y - g / 2.0));
                y -= g;
            }
        }
        let bottom = top - total;
        for (i, cx) in xs.iter().enumerate().skip(1) {
            let line = nth(&col_lines, i - 1);
            if matches!(line.as_deref(), Some("solid" | "dashed")) {
                let lx = cx - gap_x(i - 1) / 2.0;
                out.mark(
                    Mark::Line {
                        dx: 0.0,
                        dy: total,
                        t,
                    },
                    (lx, bottom),
                    c.colour,
                );
            }
        }
        for (r, ly) in row_edges {
            if matches!(nth(&row_lines, r).as_deref(), Some("solid" | "dashed")) {
                out.mark(
                    Mark::Line {
                        dx: width,
                        dy: 0.0,
                        t,
                    },
                    (0.0, ly),
                    c.colour,
                );
            }
        }
        if frame {
            let mark = Mark::Frame {
                w: width,
                h: total,
                t,
                r: 0.0,
            };
            out.mark(mark, (0.0, bottom), c.colour);
        }
        out.a = top;
        out.d = -bottom;
        out.w = width;
        out
    }

    fn enclose(&mut self, body: Bx, notation: &str, c: Ctx) -> Bx {
        let size = self.size(c);
        let words: Vec<&str> = notation
            .split([' ', ','])
            .filter(|w| !w.is_empty())
            .collect();
        let has = |w: &str| words.contains(&w);
        let mut body = body;
        if has("radical") {
            body = self.radical(body, None, c);
        }
        let t = self.k(c, K::radical_rule_thickness).max(0.03 * size);
        let pad = 0.15 * size + t;
        let boxed = [
            "box",
            "roundedbox",
            "circle",
            "left",
            "right",
            "top",
            "bottom",
            "actuarial",
            "madruwb",
            "longdiv",
        ];
        if !words
            .iter()
            .any(|w| boxed.contains(w) || w.ends_with("strike") || *w == "updiagonalarrow")
        {
            return body;
        }
        let mut out = Bx::default();
        let mut x0 = pad;
        if has("longdiv") {
            let paren = self.f.glyph(')');
            let goal = body.a + body.d + 2.0 * pad;
            let grown = paren.and_then(|g| self.f.stretch(g, goal / size, true));
            let mut curve = match (grown, paren) {
                (Some(s), _) => self.stretched(s, size, true, c.colour),
                (None, Some(g)) => self.glyph(g, size, c.colour),
                (None, None) => Bx::default(),
            };
            let mid = (body.a - body.d) / 2.0;
            curve.shift(mid - (curve.a - curve.d) / 2.0);
            let cw = curve.w;
            out.put(curve, 0.0, 0.0);
            x0 = cw + pad / 2.0;
        }
        let (top, bottom) = (body.a + pad, -(body.d + pad));
        let w = x0 + body.w + pad;
        let h = top - bottom;
        out.put(body, x0, 0.0);
        let line = |o: &mut Bx, (x, y): (f32, f32), (dx, dy): (f32, f32)| {
            o.mark(Mark::Line { dx, dy, t }, (x, y), c.colour);
        };
        let left_edge = if has("longdiv") {
            x0 - pad / 2.0
        } else {
            t / 2.0
        };
        if has("box") {
            out.mark(
                Mark::Frame {
                    w: w - t,
                    h,
                    t,
                    r: 0.0,
                },
                (t / 2.0, bottom),
                c.colour,
            );
        }
        if has("roundedbox") {
            let r = 0.25 * size;
            out.mark(
                Mark::Frame { w: w - t, h, t, r },
                (t / 2.0, bottom),
                c.colour,
            );
        }
        if has("circle") {
            let (rx, ry) = (w / 2.0 * 1.15, h / 2.0 * 1.15);
            let mark = Mark::Oval { rx, ry, t };
            out.mark(mark, (w / 2.0, (top + bottom) / 2.0), c.colour);
            out.a = out.a.max((top + bottom) / 2.0 + ry + t);
            out.d = out.d.max(ry - (top + bottom) / 2.0 + t);
        }
        if has("left") {
            line(&mut out, (t / 2.0, bottom), (0.0, h));
        }
        if has("right") || has("actuarial") || has("madruwb") {
            line(&mut out, (w - t / 2.0, bottom), (0.0, h));
        }
        if has("top") || has("actuarial") || has("longdiv") {
            line(&mut out, (left_edge, top), (w - left_edge, 0.0));
        }
        if has("bottom") || has("madruwb") {
            line(&mut out, (0.0, bottom), (w, 0.0));
        }
        if has("updiagonalstrike") || has("updiagonalarrow") {
            line(&mut out, (0.0, bottom), (w, h));
        }
        if has("downdiagonalstrike") {
            line(&mut out, (0.0, top), (w, -h));
        }
        if has("verticalstrike") {
            line(&mut out, (w / 2.0, bottom), (0.0, h));
        }
        if has("horizontalstrike") {
            line(&mut out, (0.0, (top + bottom) / 2.0), (w, 0.0));
        }
        out.a = out.a.max(top + t);
        out.d = out.d.max(-bottom + t);
        out.w = w;
        out
    }

    fn padded(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        let mut b = self.row(&n.kids, c, depth);
        let size = self.size(c);
        let (w, a, d) = (b.w, b.a, b.d);
        let value = |name: &str, now: f32| -> Option<f32> {
            let v = n.attr(name)?.trim();
            let (sign, rest) = match v.strip_prefix('+') {
                Some(r) => (1.0, r),
                None => match v.strip_prefix('-') {
                    Some(r) => (-1.0, r),
                    None => (0.0, v),
                },
            };
            let pseudo = [("width", w), ("height", a), ("depth", d)];
            let amount = pseudo.iter().find_map(|(unit, dim)| {
                let x: f32 = rest.strip_suffix(unit)?.trim().parse().ok()?;
                Some(x * dim)
            });
            let amount = amount.or_else(|| self.length(rest, size, now))?;
            Some(match sign {
                0.0 => amount,
                s => now + s * amount,
            })
        };
        let lspace = value("lspace", 0.0).unwrap_or(0.0);
        let lift = value("voffset", 0.0).unwrap_or(0.0);
        let (nw, na, nd) = (
            value("width", w).unwrap_or(w).max(0.0),
            value("height", a).unwrap_or(a),
            value("depth", d).unwrap_or(d),
        );
        for i in &mut b.items {
            i.x += lspace;
            i.y += lift;
        }
        b.w = nw;
        b.a = na;
        b.d = nd;
        b
    }

    fn fenced(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        let mo = |text: String| Node {
            name: "mo".to_owned(),
            text,
            ..Node::default()
        };
        let open = n.attr("open").unwrap_or("(").trim().to_owned();
        let close = n.attr("close").unwrap_or(")").trim().to_owned();
        let seps: Vec<char> = n
            .attr("separators")
            .unwrap_or(",")
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        let mut kids = Vec::with_capacity(n.kids.len() * 2 + 2);
        if !open.is_empty() {
            kids.push(mo(open));
        }
        for (i, k) in n.kids.iter().enumerate() {
            if i > 0
                && let Some(s) = seps.get(i - 1).or(seps.last())
            {
                kids.push(mo(s.to_string()));
            }
            kids.push(k.clone());
        }
        if !close.is_empty() {
            kids.push(mo(close));
        }
        let row = Node {
            name: "mrow".to_owned(),
            kids,
            ..Node::default()
        };
        self.row(&row.kids, c, depth)
    }

    /// Elementary math: rows right-aligned, one under another.
    fn stack(&mut self, n: &Node, c: Ctx, depth: usize) -> Bx {
        let size = self.size(c);
        let t = self.k(c, K::fraction_rule_thickness);
        let rows: Vec<Option<Bx>> = n
            .kids
            .iter()
            .take(MAX_ROWS)
            .map(|k| (k.name != "msline").then(|| self.lay(k, c, depth)))
            .collect();
        let w = rows.iter().flatten().map(|b| b.w).fold(0.0, f32::max);
        let mut out = Bx::default();
        let mut y = 0.0;
        for row in rows {
            match row {
                Some(b) => {
                    y -= b.a;
                    let (bw, bd) = (b.w, b.d);
                    out.put(b, w - bw, y);
                    y -= bd + 0.15 * size;
                }
                None => {
                    out.mark(Mark::Rect { w, h: t }, (0.0, y), c.colour);
                    y -= t + 0.15 * size;
                }
            }
        }
        let shift = self.k(c, K::axis_height) - (out.a - out.d) / 2.0;
        out.shift(shift);
        out.w = w;
        out
    }
}

/// Space and text with no operator in it, skipped when forms are set.
fn space_like(n: &Node) -> bool {
    matches!(n.name.as_str(), "mspace" | "mtext" | "none")
}

fn is_opening(text: &str) -> bool {
    matches!(text, "(" | "[" | "{" | "⟨" | "|" | "‖" | "⌈" | "⌊")
}

#[cfg(test)]
mod tests {
    use super::super::font::font;
    use super::super::tests::tree;
    use super::*;

    fn laid(markup: &str, display: bool) -> Bx {
        let f = font().expect("the math face");
        let root = tree(markup).expect("a tree");
        Layout::new(f).lay(&root, Ctx::new(display), 0)
    }

    #[test]
    fn a_fraction_stacks_round_the_axis() {
        let b = laid("<math><mfrac><mi>a</mi><mi>b</mi></mfrac></math>", false);
        let rules = b
            .items
            .iter()
            .filter(|i| matches!(i.mark, Mark::Rect { .. }))
            .count();
        assert_eq!(rules, 1);
        assert!(b.a > 0.6 && b.d > 0.3, "{} {}", b.a, b.d);
        let shown = laid(
            "<math display=\"block\"><mfrac><mi>a</mi><mi>b</mi></mfrac></math>",
            true,
        );
        assert!(shown.a > b.a, "display style sets it taller");
    }

    #[test]
    fn scripts_shrink_and_rise() {
        let b = laid("<math><msup><mi>x</mi><mn>2</mn></msup></math>", false);
        let sizes: Vec<f32> = b
            .items
            .iter()
            .filter_map(|i| match i.mark {
                Mark::Glyph { size, .. } => Some(size),
                _ => None,
            })
            .collect();
        assert_eq!(sizes.len(), 2);
        assert!(sizes.get(1).is_some_and(|s| *s < 0.8));
        let two = b.items.get(1).map_or(0.0, |i| i.y);
        assert!(two > 0.3, "{two}");
    }

    #[test]
    fn fences_grow_to_what_they_hold() {
        let plain = laid(
            "<math><mrow><mo>(</mo><mi>x</mi><mo>)</mo></mrow></math>",
            false,
        );
        let tall = laid(
            "<math><mrow><mo>(</mo><mfrac><mfrac><mi>a</mi><mi>b</mi></mfrac><mi>c</mi></mfrac><mo>)</mo></mrow></math>",
            true,
        );
        let first = |b: &Bx| b.items.first().map(|i| i.mark);
        assert_ne!(first(&plain), first(&tall), "a larger paren is used");
        assert!(tall.a + tall.d > 2.0 * (plain.a + plain.d));
    }

    #[test]
    fn operators_are_spaced_by_kind() {
        let sum = laid("<math><mi>a</mi><mo>+</mo><mi>b</mi></math>", false);
        let neg = laid("<math><mo>-</mo><mi>b</mi></math>", false);
        let xs: Vec<f32> = sum.items.iter().map(|i| i.x).collect();
        let gap = xs.get(1).copied().unwrap_or(0.0) - xs.first().copied().unwrap_or(0.0);
        assert!(gap > 0.6, "binary plus is spaced: {gap}");
        let start = neg.items.first().map_or(1.0, |i| i.x);
        assert!(start.abs() < 0.01, "a leading minus is unary");
    }

    #[test]
    fn a_root_draws_its_bar_over_the_body() {
        let b = laid("<math><msqrt><mi>x</mi></msqrt></math>", false);
        let bar = b.items.iter().find(|i| matches!(i.mark, Mark::Rect { .. }));
        assert!(bar.is_some_and(|r| r.y > 0.4));
        let cube = laid("<math><mroot><mi>x</mi><mn>3</mn></mroot></math>", false);
        assert!(cube.w > b.w);
    }

    #[test]
    fn limits_stack_in_display_and_sit_beside_inline() {
        let src = "<munderover><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></munderover>";
        let shown = laid(&format!("<math display=\"block\">{src}</math>"), true);
        let inline = laid(&format!("<math>{src}</math>"), false);
        assert!(shown.w < inline.w + 0.2 || shown.a > inline.a);
        assert!(shown.a + shown.d > inline.a + inline.d);
    }

    #[test]
    fn a_table_centres_on_the_axis() {
        let b = laid(
            "<math><mtable><mtr><mtd><mn>1</mn></mtd><mtd><mn>0</mn></mtd></mtr>\
             <mtr><mtd><mn>0</mn></mtd><mtd><mn>1</mn></mtd></mtr></mtable></math>",
            false,
        );
        let glyphs = b
            .items
            .iter()
            .filter(|i| matches!(i.mark, Mark::Glyph { .. }))
            .count();
        assert_eq!(glyphs, 4);
        assert!((b.a - b.d - 2.0 * 0.258).abs() < 0.05, "{} {}", b.a, b.d);
    }

    #[test]
    fn enclosures_and_phantoms() {
        let boxed = laid(
            "<math><menclose notation=\"box\"><mi>x</mi></menclose></math>",
            false,
        );
        assert!(
            boxed
                .items
                .iter()
                .any(|i| matches!(i.mark, Mark::Frame { .. }))
        );
        let ghost = laid("<math><mphantom><mi>x</mi></mphantom></math>", false);
        assert!(ghost.items.is_empty() && ghost.w > 0.3);
    }
}
