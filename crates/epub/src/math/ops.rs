//! The operator dictionary: how `mo` text is spaced, grown and set.

/// Where an operator sits in its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    Prefix,
    Infix,
    Postfix,
}

impl Form {
    pub(crate) fn parse(v: &str) -> Option<Self> {
        match v.trim() {
            "prefix" => Some(Self::Prefix),
            "infix" => Some(Self::Infix),
            "postfix" => Some(Self::Postfix),
            _ => None,
        }
    }
}

/// An operator's kind, which sets its spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Class {
    #[default]
    Other,
    Fence,
    Relation,
    Binary,
    Punct,
    Large,
    Invisible,
}

/// How an operator is set, its spaces in ems.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Op {
    pub(crate) class: Class,
    pub(crate) lspace: f32,
    pub(crate) rspace: f32,
    /// Grows to fit what it fences, or what it sits over.
    pub(crate) stretchy: bool,
    pub(crate) largeop: bool,
    /// Limits go beside it, as scripts, outside display style.
    pub(crate) movable: bool,
    /// Sits close over its base, as an accent does.
    pub(crate) accent: bool,
}

const FENCES: &str = "()[]{}|‖⟨⟩⌈⌉⌊⌋⟦⟧⟮⟯〈〉⦃⦄⦇⦈⟪⟫";
const RELATIONS: &str =
    "=<>≤≥≠≈≡≢≅≆∼≃∝∈∉∋∌⊂⊃⊆⊇⊄⊅⊊⊋≪≫≺≻⪯⪰⊥∥∦∣∤→←↔⇒⇐⇔↦⟶⟵⟷⟹⟸⟺↑↓↗↘↖↙↪↩⇌⇋⊢⊣⊨⊩≔≕≐≜≟≲≳≶≷≍∷:⊏⊐⊑⊒⋖⋗≦≧⩽⩾∽≀⟼⇝↝↠↣";
const BINARY: &str = "+−-±∓×÷·⋅∗∘∙∪∩⊕⊖⊗⊘⊙∧∨∖⊎⊓⊔⋆★†‡⨯⊞⊠⊡⋄◇△▽⊲⊳⋉⋊⋈⊼⊻⊛⊚⨿⋒⋓";
const LIMITS: &str = "∑∏∐⋃⋂⨀⨁⨂⨄⨅⨆⋁⋀";
const INTEGRALS: &str = "∫∬∭∮∯∰∱∲∳⨌⨍⨏⨑";
const PUNCT: &str = ",;";
const INVISIBLE: &str = "\u{2061}\u{2062}\u{2063}\u{2064}";
/// Grow across to the width of what they sit over or under.
const ACROSS: &str = "‾¯_→←↔⇒⇐⇔⏞⏟⏜⏝⎴⎵^ˆ~˜ˇ⟶⟵⟷↼⇀↽⇁⇄⇆⥊⥋";
/// Sit close over their base by default.
const ACCENTS: &str = "‾¯^ˆ~˜ˇ˘˙¨´`⏞⏟⏜⏝⎴⎵⃗\u{2D9}\u{2DA}";
/// Words set as limits beside or under, as `lim` is.
const LIMIT_WORDS: &[&str] = &[
    "lim", "max", "min", "sup", "inf", "limsup", "liminf", "det", "gcd", "Pr",
];

/// Spaces in eighteenths of an em, as the dictionary gives them.
const THIN: f32 = 3.0 / 18.0;
const MEDIUM: f32 = 4.0 / 18.0;
const THICK: f32 = 5.0 / 18.0;
const VERY_THIN: f32 = 1.0 / 18.0;

/// What the dictionary says of `text` in `form`.
pub(crate) fn lookup(text: &str, form: Form) -> Op {
    let mut chars = text.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return word(text);
    };
    let has = |set: &str| set.contains(c);
    let mut op = Op {
        accent: has(ACCENTS),
        stretchy: has(ACROSS),
        ..Op::default()
    };
    if has(INVISIBLE) {
        op.class = Class::Invisible;
    } else if has(FENCES) {
        op.class = Class::Fence;
        op.stretchy = true;
        if form == Form::Infix && matches!(c, '|' | '‖') {
            (op.lspace, op.rspace) = (THICK, THICK);
        }
    } else if has(LIMITS) || has(INTEGRALS) {
        op.class = Class::Large;
        op.largeop = true;
        op.movable = has(LIMITS);
        (op.lspace, op.rspace) = (VERY_THIN, THIN);
    } else if has(RELATIONS) {
        op.class = Class::Relation;
        (op.lspace, op.rspace) = (THICK, THICK);
    } else if has(BINARY) {
        op.class = Class::Binary;
        if form == Form::Infix {
            (op.lspace, op.rspace) = (MEDIUM, MEDIUM);
        }
    } else if has(PUNCT) {
        op.class = Class::Punct;
        op.rspace = THIN;
    } else if form == Form::Infix && !has("./\\…⋯⋮⋱⋰!′″‴'") && !op.accent {
        // Unknown: the dictionary's default is a thick space each side.
        (op.lspace, op.rspace) = (THICK, THICK);
    }
    op
}

/// A word in `mo`, as `lim` or `sin`: a prefix with a thin space after.
fn word(text: &str) -> Op {
    Op {
        rspace: if text.is_empty() { 0.0 } else { THIN },
        movable: LIMIT_WORDS.contains(&text),
        ..Op::default()
    }
}

/// A length MathML names, as `thinmathspace`, in ems.
pub(crate) fn named_space(v: &str) -> Option<f32> {
    let (negative, name) = match v.strip_prefix("negative") {
        Some(rest) => (true, rest),
        None => (false, v),
    };
    let eighteenths = match name {
        "veryverythinmathspace" => 1.0,
        "verythinmathspace" => 2.0,
        "thinmathspace" => 3.0,
        "mediummathspace" => 4.0,
        "thickmathspace" => 5.0,
        "verythickmathspace" => 6.0,
        "veryverythickmathspace" => 7.0,
        _ => return None,
    };
    let em = eighteenths / 18.0;
    Some(if negative { -em } else { em })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_space_by_kind() {
        let eq = lookup("=", Form::Infix);
        assert_eq!((eq.class, eq.lspace), (Class::Relation, THICK));
        assert_eq!(lookup("+", Form::Infix).lspace, MEDIUM);
        assert_eq!(lookup("−", Form::Prefix).lspace, 0.0);
        assert!(lookup("(", Form::Prefix).stretchy);
        let sum = lookup("∑", Form::Prefix);
        assert!(sum.largeop && sum.movable);
        assert!(!lookup("∫", Form::Prefix).movable);
        assert_eq!(lookup(",", Form::Infix).rspace, THIN);
        assert!(lookup("lim", Form::Prefix).movable);
        assert!(lookup("^", Form::Postfix).accent);
        assert_eq!(named_space("negativethinmathspace"), Some(-THIN));
    }
}
