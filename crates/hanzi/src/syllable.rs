//! One pinyin syllable, tone marks and all, to Kokoro's phonemes.

/// Initials as misaki's table hears them; the two-letter ones first.
const INITIALS: [(&str, &str); 21] = [
    ("zh", "ꭧ"),
    ("ch", "ꭧʰ"),
    ("sh", "ʂ"),
    ("b", "p"),
    ("c", "ʦʰ"),
    ("d", "t"),
    ("f", "f"),
    ("g", "k"),
    ("h", "x"),
    ("j", "ʨ"),
    ("k", "kʰ"),
    ("l", "l"),
    ("m", "m"),
    ("n", "n"),
    ("p", "pʰ"),
    ("q", "ʨʰ"),
    ("r", "ɻ"),
    ("s", "s"),
    ("t", "tʰ"),
    ("x", "ɕ"),
    ("z", "ʦ"),
];

/// Finals in their full spelling; `0` is where the tone goes.
const FINALS: [(&str, &str); 35] = [
    ("a", "a0"),
    ("ai", "ai0"),
    ("an", "a0n"),
    ("ang", "a0ŋ"),
    ("ao", "au0"),
    ("e", "ɤ0"),
    ("ei", "ei0"),
    ("en", "ə0n"),
    ("eng", "ə0ŋ"),
    ("i", "i0"),
    ("ia", "ja0"),
    ("ian", "jɛ0n"),
    ("iang", "ja0ŋ"),
    ("iao", "jau0"),
    ("ie", "je0"),
    ("in", "i0n"),
    ("iou", "jou0"),
    ("ing", "i0ŋ"),
    ("iong", "jʊ0ŋ"),
    ("ong", "ʊ0ŋ"),
    ("ou", "ou0"),
    ("u", "u0"),
    ("uei", "wei0"),
    ("ua", "wa0"),
    ("uai", "wai0"),
    ("uan", "wa0n"),
    ("uen", "wə0n"),
    ("uang", "wa0ŋ"),
    ("ueng", "wə0ŋ"),
    ("uo", "wo0"),
    ("o", "wo0"),
    ("ü", "y0"),
    ("üe", "ɥe0"),
    ("üan", "ɥɛ0n"),
    ("ün", "y0n"),
];

/// Syllables with no initial and final: interjections and lone nasals.
const WHOLE: [(&str, &str); 9] = [
    ("io", "jɔ0"),
    ("ê", "ɛ0"),
    ("er", "ɚ0"),
    ("o", "ɔ0"),
    ("hm", "hm0"),
    ("hng", "hŋ0"),
    ("m", "m0"),
    ("n", "n0"),
    ("ng", "ŋ0"),
];

/// Kokoro's phonemes for `syllable`, as `zhōng`; false if it isn't one.
pub fn push(syllable: &str, out: &mut String) -> bool {
    let (plain, tone) = untone(syllable);
    let Some((initial, fin)) = sounds(&plain) else {
        return false;
    };
    let tone = match tone {
        1 => "→",
        2 => "↗",
        3 => "↓",
        4 => "↘",
        _ => "",
    };
    out.push_str(initial);
    for c in fin.chars() {
        match c {
            '0' => out.push_str(tone),
            c => out.push(c),
        }
    }
    true
}

/// The letters without tone marks, and the tone; 5 when unmarked.
fn untone(syllable: &str) -> (String, u8) {
    let mut plain = String::with_capacity(syllable.len());
    let mut tone = 5;
    for c in syllable.chars() {
        let (base, mark) = match c {
            'ā' => ('a', 1),
            'á' => ('a', 2),
            'ǎ' => ('a', 3),
            'à' => ('a', 4),
            'ē' => ('e', 1),
            'é' => ('e', 2),
            'ě' => ('e', 3),
            'è' => ('e', 4),
            'ī' => ('i', 1),
            'í' => ('i', 2),
            'ǐ' => ('i', 3),
            'ì' => ('i', 4),
            'ō' => ('o', 1),
            'ó' => ('o', 2),
            'ǒ' => ('o', 3),
            'ò' => ('o', 4),
            'ū' => ('u', 1),
            'ú' => ('u', 2),
            'ǔ' => ('u', 3),
            'ù' => ('u', 4),
            'ǖ' => ('ü', 1),
            'ǘ' => ('ü', 2),
            'ǚ' => ('ü', 3),
            'ǜ' => ('ü', 4),
            'ń' => ('n', 2),
            'ň' => ('n', 3),
            'ǹ' => ('n', 4),
            'ḿ' => ('m', 2),
            'ế' => ('ê', 2),
            'ề' => ('ê', 4),
            'v' => ('ü', 0),
            '\u{304}' => ('\0', 1),
            '\u{301}' => ('\0', 2),
            '\u{30c}' => ('\0', 3),
            '\u{300}' => ('\0', 4),
            c => (c, 0),
        };
        if base != '\0' {
            plain.push(base);
        }
        if mark != 0 {
            tone = mark;
        }
    }
    (plain, tone)
}

/// The initial's phonemes and the final's pattern, as misaki splits them.
fn sounds(plain: &str) -> Option<(&'static str, &'static str)> {
    if let Some(&(_, whole)) = WHOLE.iter().find(|(p, _)| *p == plain) {
        return Some(("", whole));
    }
    let (initial, ipa, rest) = INITIALS
        .iter()
        .find_map(|&(p, ipa)| plain.strip_prefix(p).map(|rest| (p, ipa, rest)))
        .unwrap_or(("", "", plain));
    let fin = full_final(initial, rest);
    let apical = matches!(initial, "zh" | "ch" | "sh" | "r" | "z" | "c" | "s");
    let pattern = match (apical, fin.as_str()) {
        (true, "i") => "ɨ0",
        (_, fin) => FINALS.iter().find(|(p, _)| *p == fin)?.1,
    };
    Some((ipa, pattern))
}

/// The final as the pinyin scheme spells it in full, as `iou` for `-iu`.
fn full_final(initial: &str, rest: &str) -> String {
    if initial.is_empty() {
        // pypinyin reads `yo` as `o` and `wong` as `ong`.
        match rest {
            "yo" => return "o".into(),
            "wong" => return "ong".into(),
            _ => {}
        }
        let pairs = [
            ("yu", "ü"),
            ("yi", "i"),
            ("y", "i"),
            ("wu", "u"),
            ("w", "u"),
        ];
        for (from, to) in pairs {
            if let Some(tail) = rest.strip_prefix(from) {
                return format!("{to}{tail}");
            }
        }
        return rest.into();
    }
    if matches!(initial, "j" | "q" | "x")
        && let Some(tail) = rest.strip_prefix('u')
    {
        return format!("ü{tail}");
    }
    match rest {
        "iu" => "iou".into(),
        "ui" => "uei".into(),
        "un" => "uen".into(),
        rest => rest.into(),
    }
}
