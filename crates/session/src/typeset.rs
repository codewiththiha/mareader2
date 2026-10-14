//! The reader's text settings as the typography books are set in.
use mareader_reflow::{FACES, Faces, Family, Kind, Typography};
use mareader_settings::{FontChoice, FontFamily, TextSettings};

/// The body takes the serif slot until a face of its own is picked.
pub fn typography(t: &TextSettings) -> Typography {
    let body = match &t.default_font {
        FontChoice::Default => stack(&t.serif_font, Kind::Serif),
        pick => stack(pick, kind_of(pick).unwrap_or(Kind::Serif)),
    };
    Typography {
        font_size: t.font_size,
        line_height: t.line_height,
        block_gap: t.paragraph_margin,
        weight: t.font_weight,
        word_spacing: t.word_spacing,
        letter_spacing: t.letter_spacing,
        indent: t.text_indent,
        justify: t.justify,
        hyphenate: t.hyphenation,
        book_layout: t.book_layout,
        faces: Faces {
            body,
            heading: stack(&t.sans_font, Kind::Sans),
            mono: stack(&t.mono_font, Kind::Mono),
        },
        ..Typography::default()
    }
}

/// The bundled face behind a slot's id, if it ships.
pub fn bundled(id: &str) -> Option<&'static mareader_reflow::Face> {
    FACES.iter().find(|f| f.id == id)
}

fn kind_of(choice: &FontChoice) -> Option<Kind> {
    match choice {
        FontChoice::Default => None,
        FontChoice::System(f) => Some(match f.family() {
            FontFamily::Serif => Kind::Serif,
            FontFamily::Sans => Kind::Sans,
            FontFamily::Mono => Kind::Mono,
        }),
        FontChoice::Bundled(id) => bundled(id).map(|f| f.kind),
    }
}

/// A slot's pick, then the bundled face of its kind as a last resort.
fn stack(choice: &FontChoice, slot: Kind) -> Vec<Family> {
    let mut stack: Vec<Family> = match choice {
        FontChoice::Default => Vec::new(),
        FontChoice::System(f) => f.names().iter().map(|n| family(n)).collect(),
        FontChoice::Bundled(id) => bundled(id)
            .map(|f| Family::Named(f.name.to_owned()))
            .into_iter()
            .collect(),
    };
    let last = Family::Named(slot.bundled().to_owned());
    if !stack.contains(&last) {
        stack.push(last);
    }
    stack
}

/// CSS generic names mean the system's face of that kind.
fn family(name: &str) -> Family {
    match name {
        "serif" => Family::Generic(Kind::Serif),
        "sans-serif" => Family::Generic(Kind::Sans),
        "monospace" => Family::Generic(Kind::Mono),
        named => Family::Named(named.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use mareader_settings::SystemFont;

    use super::*;

    fn named(n: &str) -> Family {
        Family::Named(n.to_owned())
    }

    #[test]
    fn defaults_set_the_bundled_faces_as_before() {
        let t = typography(&TextSettings::default());
        assert_eq!(t, Typography::default());
    }

    #[test]
    fn every_knob_reaches_the_typography() {
        let s = TextSettings {
            font_size: 21.0,
            font_weight: 500.0,
            line_height: 2.0,
            paragraph_margin: 0.5,
            word_spacing: 2.0,
            letter_spacing: 0.05,
            text_indent: 1.5,
            justify: true,
            hyphenation: true,
            book_layout: true,
            ..TextSettings::default()
        };
        let t = typography(&s);
        assert_eq!(
            (t.font_size, t.weight, t.line_height, t.block_gap),
            (21.0, 500.0, 2.0, 0.5)
        );
        assert_eq!(
            (t.word_spacing, t.letter_spacing, t.indent),
            (2.0, 0.05, 1.5)
        );
        assert!(t.justify && t.hyphenate && t.book_layout);
    }

    #[test]
    fn picks_lead_their_stacks_and_bundled_faces_back_them() {
        let s = TextSettings {
            serif_font: FontChoice::System(SystemFont::Georgia),
            sans_font: FontChoice::System(SystemFont::UiSans),
            mono_font: FontChoice::Bundled("jetbrains-mono".into()),
            ..TextSettings::default()
        };
        let f = typography(&s).faces;
        // The body follows the serif slot.
        assert_eq!(f.body, [named("Georgia"), named("Literata")]);
        assert_eq!(f.heading, [Family::Generic(Kind::Sans), named("Inter")]);
        assert_eq!(f.mono, [named("JetBrains Mono")]);
        let s = TextSettings {
            default_font: FontChoice::System(SystemFont::Verdana),
            ..s
        };
        assert_eq!(
            typography(&s).faces.body,
            [named("Verdana"), named("Inter")]
        );
        // An id that doesn't ship is the slot's default.
        let s = TextSettings {
            default_font: FontChoice::Bundled("gone".into()),
            ..TextSettings::default()
        };
        assert_eq!(typography(&s).faces.body, [named("Literata")]);
    }
}
