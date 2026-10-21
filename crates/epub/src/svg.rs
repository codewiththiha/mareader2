//! Inline SVG written back as XML from its tokens, drawn as one picture.
use html5ever::tokenizer::{Tag, TagKind, Token};

/// A drawing past this much markup is dropped.
const MAX_MARKUP: usize = 8 << 20;
/// At most this many linked images per drawing.
pub(crate) const MAX_LINKS: usize = 64;

/// What draws by itself; a drawing of only images is a picture's frame.
const SHAPES: &[&str] = &[
    "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text", "use", "tspan",
    "textpath",
];

/// SVG element names the HTML tokenizer lowercases, in their own case.
const ELEMENTS: &[&str] = &[
    "altGlyph",
    "altGlyphDef",
    "altGlyphItem",
    "animateColor",
    "animateMotion",
    "animateTransform",
    "clipPath",
    "feBlend",
    "feColorMatrix",
    "feComponentTransfer",
    "feComposite",
    "feConvolveMatrix",
    "feDiffuseLighting",
    "feDisplacementMap",
    "feDistantLight",
    "feDropShadow",
    "feFlood",
    "feFuncA",
    "feFuncB",
    "feFuncG",
    "feFuncR",
    "feGaussianBlur",
    "feImage",
    "feMerge",
    "feMergeNode",
    "feMorphology",
    "feOffset",
    "fePointLight",
    "feSpecularLighting",
    "feSpotLight",
    "feTile",
    "feTurbulence",
    "foreignObject",
    "glyphRef",
    "linearGradient",
    "radialGradient",
    "textPath",
];

/// SVG attribute names the HTML tokenizer lowercases, in their own case.
const ATTRIBUTES: &[&str] = &[
    "attributeName",
    "attributeType",
    "baseFrequency",
    "baseProfile",
    "calcMode",
    "clipPathUnits",
    "diffuseConstant",
    "edgeMode",
    "filterUnits",
    "glyphRef",
    "gradientTransform",
    "gradientUnits",
    "kernelMatrix",
    "kernelUnitLength",
    "keyPoints",
    "keySplines",
    "keyTimes",
    "lengthAdjust",
    "limitingConeAngle",
    "markerHeight",
    "markerUnits",
    "markerWidth",
    "maskContentUnits",
    "maskUnits",
    "numOctaves",
    "pathLength",
    "patternContentUnits",
    "patternTransform",
    "patternUnits",
    "pointsAtX",
    "pointsAtY",
    "pointsAtZ",
    "preserveAlpha",
    "preserveAspectRatio",
    "primitiveUnits",
    "refX",
    "refY",
    "repeatCount",
    "repeatDur",
    "requiredExtensions",
    "requiredFeatures",
    "specularConstant",
    "specularExponent",
    "spreadMethod",
    "startOffset",
    "stdDeviation",
    "stitchTiles",
    "surfaceScale",
    "systemLanguage",
    "tableValues",
    "targetX",
    "targetY",
    "textLength",
    "viewBox",
    "viewTarget",
    "xChannelSelector",
    "yChannelSelector",
    "zoomAndPan",
];

/// An `svg` element being read.
#[derive(Debug, Default)]
pub(crate) struct Capture {
    pub(crate) markup: String,
    depth: usize,
    /// Its `image` references, as written.
    pub(crate) images: Vec<String>,
    /// Shapes or text of its own, beyond images.
    pub(crate) drawn: bool,
    /// Too large to keep; read to its end and dropped.
    pub(crate) broken: bool,
}

impl Capture {
    /// Starts at the root `svg` start tag `tag`.
    pub(crate) fn new(tag: &Tag) -> Self {
        let mut c = Self::default();
        c.open(tag, true);
        c
    }

    /// Takes the next token; `true` once the root's end tag is read.
    pub(crate) fn token(&mut self, token: &Token) -> bool {
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => self.open(tag, false),
            Token::TagToken(tag) => {
                self.depth = self.depth.saturating_sub(1);
                let end = format!("</{}>", element(&tag.name));
                self.push(&end);
                return self.depth == 0;
            }
            Token::CharacterTokens(text) => self.push(&escape(text, false)),
            Token::CommentToken(c) => {
                // CDATA reads as a comment here; style sheets hide in it.
                let inner = c.strip_prefix("[CDATA[").and_then(|c| c.strip_suffix("]]"));
                if let Some(inner) = inner {
                    self.push(&escape(inner, false));
                }
            }
            Token::EOFToken => return true,
            _ => {}
        }
        false
    }

    fn open(&mut self, tag: &Tag, root: bool) {
        let name = element(&tag.name);
        if name == "image" {
            let href = tag.attrs.iter().find(|a| {
                let n: &str = &a.name.local;
                n == "href" || n == "xlink:href"
            });
            if let Some(href) = href.filter(|_| self.images.len() < MAX_LINKS) {
                self.images.push(href.value.trim().to_owned());
            }
        }
        self.drawn |= SHAPES.contains(&name.to_ascii_lowercase().as_str());
        let mut out = format!("<{name}");
        for a in &tag.attrs {
            let n: &str = &a.name.local;
            // Namespaces are ours to declare; unknown prefixes aren't XML.
            let known = match n.split_once(':') {
                Some((prefix, _)) => matches!(prefix, "xlink" | "xml"),
                None => n != "xmlns",
            };
            if known {
                let value = escape(&a.value, true);
                out.push_str(&format!(" {}=\"{value}\"", attribute(n)));
            }
        }
        if root {
            out.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
            out.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
        }
        if tag.self_closing {
            out.push_str("/>");
        } else {
            out.push('>');
            self.depth += 1;
        }
        self.push(&out);
    }

    fn push(&mut self, s: &str) {
        if self.markup.len() + s.len() > MAX_MARKUP {
            self.broken = true;
        }
        if !self.broken {
            self.markup.push_str(s);
        }
    }
}

/// Whether `name` starts an SVG drawing, prefixed or not.
pub(crate) fn is_root(name: &str) -> bool {
    name.rsplit(':').next() == Some("svg")
}

/// An element's name without its prefix, in SVG's case.
fn element(name: &str) -> &str {
    let local = name.rsplit(':').next().unwrap_or(name);
    cased(local, ELEMENTS)
}

fn attribute(name: &str) -> &str {
    cased(name, ATTRIBUTES)
}

fn cased<'a>(name: &'a str, table: &[&'static str]) -> &'a str {
    match table.iter().find(|t| t.eq_ignore_ascii_case(name)) {
        Some(t) => t,
        None => name,
    }
}

fn escape(text: &str, quoted: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if quoted => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}
