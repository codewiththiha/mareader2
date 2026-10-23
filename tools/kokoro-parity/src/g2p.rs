//! mareader-g2p against loqui-g2p 0.4.0, the crate it ports, input by input.

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use loqui_g2p as loqui;
use mareader_g2p as ours;

use crate::R;

/// Public-domain openings: timing, and words in context.
const PROSE: &[&str] = &[
    "It is a truth universally acknowledged, that a single man in possession of a good \
     fortune, must be in want of a wife. However little known the feelings or views of \
     such a man may be on his first entering a neighbourhood, this truth is so well fixed \
     in the minds of the surrounding families, that he is considered the rightful \
     property of some one or other of their daughters.",
    "Alice was beginning to get very tired of sitting by her sister on the bank, and of \
     having nothing to do: once or twice she had peeped into the book her sister was \
     reading, but it had no pictures or conversations in it, 'and what is the use of a \
     book,' thought Alice 'without pictures or conversations?'",
    "It was the best of times, it was the worst of times, it was the age of wisdom, it \
     was the age of foolishness, it was the epoch of belief, it was the epoch of \
     incredulity, it was the season of Light, it was the season of Darkness.",
    "Call me Ishmael. Some years ago—never mind how long precisely—having little or no \
     money in my purse, and nothing particular to interest me on shore, I thought I \
     would sail about a little and see the watery part of the world.",
    "To Sherlock Holmes she is always the woman. I have seldom heard him mention her \
     under any other name. In his eyes she eclipses and predominates the whole of her sex.",
    "You will rejoice to hear that no disaster has accompanied the commencement of an \
     enterprise which you have regarded with such evil forebodings. I arrived here \
     yesterday, and my first task is to assure my dear sister of my welfare.",
];

/// Heteronyms, abbreviations, markup, symbols and other corners.
const SENTENCES: &[&str] = &[
    "I read the book yesterday, and I will read it again tomorrow.",
    "They record a new record every year.",
    "The wind was too strong to wind the sail.",
    "Please don't close the door; it's too close to the stairs.",
    "Dr. Smith lives on Baker St. near the U.K. border, I think.",
    "She said, \"I can't believe it's already 9:30!\"",
    "Visit https://example.com/docs or email ana@example.org for help.",
    "The meeting is on March 3rd, 2027, at 10:45 a.m.",
    "NASA and the FBI used AI to find the USB drive.",
    "He paid $1,234.56 for 3 tickets, i.e. about $411.52 each.",
    "Temperatures fell to -5 degrees, a 73% drop.",
    "Wi-Fi, e-mail and co-operate are hyphenated words.",
    "The café served crème brûlée and jalapeño nachos.",
    "“Curly quotes” and ‘single ones’ — plus an em dash… and ellipses.",
    "Mr. and Mrs. O'Brien's children went to St. Mary's.",
    "It's the 1990s, rock 'n' roll is everywhere, and y'all know it.",
    "[Kokoro](/kˈOkəɹO/) speaks, and [read](/ɹˈɛd/) is past.",
    "I said [never](+2) and meant it [once](-1), [5](#a#) times.",
    "Call 555-0123 or 1-800-555-0199 before 5pm.",
    "Chapter IV: The Return of Henry VIII.",
    "Use config.toml, not settings.json, in version 2.0.1.",
    "The ratio was 3:2 and the score 21-14.",
    "Read pp. 12-15, vol. 3, ed. 2.",
    "He used to live there; he used it daily.",
    "Live music is live tonight; they live nearby.",
    "The lead singer held a lead pipe.",
    "Bass fish and bass guitars are different.",
    "Tear the paper without shedding a tear.",
    "An honest hour, a European union, a one-time offer.",
    "To be, or not to be: that is the question.",
    "Why? Because! Really?! Yes...",
    "The quick brown fox jumps over the lazy dog.",
    "Zorbulating quixotic flibbertigibbets confound xylophonists.",
    "Schadenfreude, déjà vu, and zeitgeist are borrowed words.",
    "Tchaikovsky, Nietzsche, Dvořák and Saint-Saëns.",
    "Their 2nd and 23rd attempts cost £3.50 and €0.99.",
    "Approximately 1.5 million people, or 0.02%, agreed.",
    "Text with  double  spaces and\ttabs\nand new lines.",
    "ALL CAPS SHOUTING IS HARD TO READ.",
    "iPhone, eBay, McDonald's and JavaScript.",
    "The year 2000 bug, the 1800s, and the year 1066.",
    "Is it 4.70 or 4.7? Either way, it's under 5.",
    "A B C D E F G.",
    "日本語 and 中文 are not English.",
    "Numbers like 007, 0.5, and 10,000,000 are tricky.",
    "Rs. 500, ¥1000 and 50¢ are other currencies.",
    "& + @ % # symbols appear too.",
    "",
    "   ",
];

const SYLLABLES: &[&str] = &[
    "ka", "zor", "bu", "late", "fen", "drix", "mo", "quil", "sha", "ther", "on", "ix", "ple",
    "gra", "vy", "stro", "nae", "wick", "el", "tor",
];

/// Mismatches listed in the report, per dialect.
const SHOWN: usize = 12;

fn ordinal(n: u64) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Thousands separated by commas, as "1,234,567".
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Words no lexicon has, from a fixed seed, so the neural model answers.
fn made_up(count: usize) -> Vec<String> {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = |n: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as usize % n
    };
    (0..count)
        .map(|i| {
            let parts = 2 + next(3);
            let word: String = (0..parts)
                .map(|_| {
                    SYLLABLES
                        .get(next(SYLLABLES.len()))
                        .copied()
                        .unwrap_or("ka")
                })
                .collect();
            match i % 4 {
                0 => {
                    let mut c = word.chars();
                    c.next()
                        .map(|f| f.to_uppercase().chain(c).collect())
                        .unwrap_or_default()
                }
                _ => word,
            }
        })
        .collect()
}

fn corpus(dir: &Path, dialect: ours::Dialect) -> R<Vec<String>> {
    let mut inputs: Vec<String> = PROSE
        .iter()
        .chain(SENTENCES)
        .map(|s| s.to_string())
        .collect();
    for n in (0..=1100).chain((1100..=1_000_000).step_by(997)) {
        inputs.push(n.to_string());
    }
    for n in [1_234_567, 1_000_000_000, 987_654_321_012, 20_000, 1_001] {
        inputs.push(n.to_string());
        inputs.push(grouped(n));
    }
    inputs.extend((1000..=2100).map(|y| format!("In {y}, it rained.")));
    inputs.extend((0..=300).map(ordinal));
    for d in [
        "0.5", "3.14", "12.50", "4.70", "100.001", "7.0", ".5", "3.14.15",
    ] {
        inputs.push(d.to_owned());
    }
    for symbol in ["$", "£", "€"] {
        for amount in [
            "1", "1.00", "0.99", "12.50", "1,234.56", "2.5", "1.01", "100", "0.5",
        ] {
            inputs.push(format!("{symbol}{amount}"));
            inputs.push(format!("It costs {symbol}{amount}."));
        }
    }
    for other in [
        "-5",
        "-12.75",
        "73%",
        "555-0123",
        "9:30",
        "10:05 AM",
        "1990s",
        "the 80s",
        "3rd-place",
        "B22",
        "4x4",
        "#1",
        "No. 7",
        "v2.0",
        "1/2",
        "24/7",
        "100km",
        "5kg",
        "60mph",
        "2s",
        "3's",
        "10ed",
        "1st-ing",
    ] {
        inputs.push(other.to_owned());
    }
    let [gold, silver, _] = dialect.files();
    for file in [gold, silver] {
        let words: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(&std::fs::read(dir.join(file))?)?;
        let keys: Vec<&String> = words.keys().collect();
        inputs.extend(keys.iter().step_by(3).map(|k| (*k).clone()));
        for k in keys.iter().step_by(41) {
            inputs.push(k.to_uppercase());
            inputs.push(format!("{k}s"));
            inputs.push(format!("{k}ed"));
            inputs.push(format!("{k}ing"));
            inputs.push(format!("The {k} was here."));
        }
    }
    inputs.extend(made_up(400));
    Ok(inputs)
}

fn timed(f: impl Fn()) -> Duration {
    let start = Instant::now();
    f();
    start.elapsed()
}

type Tokens = Vec<(String, String, bool)>;

/// Adds a section on both dialects to `report`, failures to `failed`.
pub fn check(dir: &Path, report: &mut String, failed: &mut Vec<String>) -> R<()> {
    writeln!(report, "## G2P parity: mareader-g2p vs loqui-g2p 0.4.0\n")?;
    let mut rows = String::new();
    let mut shown = String::new();
    let dialects = [
        (
            ours::Dialect::American,
            loqui::Dialect::American,
            "American",
        ),
        (ours::Dialect::British, loqui::Dialect::British, "British"),
    ];
    for (dialect, theirs_dialect, name) in dialects {
        let start = Instant::now();
        let mine = ours::G2p::load(dir, dialect)?;
        let our_load = start.elapsed();
        let start = Instant::now();
        let theirs = loqui::G2p::new(theirs_dialect, loqui::OovFallback::Neural)?;
        let ref_load = start.elapsed();

        let our_prose = timed(|| {
            PROSE
                .iter()
                .for_each(|p| drop(black_box(mine.phonemize(p))))
        });
        let ref_prose = timed(|| {
            PROSE
                .iter()
                .for_each(|p| drop(black_box(theirs.phonemize(p))))
        });

        let inputs = corpus(dir, dialect)?;
        let (mut same_text, mut same_tokens, mut listed) = (0, 0, 0);
        let started = Instant::now();
        for input in &inputs {
            let (a, b) = (mine.phonemize(input), theirs.phonemize(input));
            let ta: Tokens = mine
                .tokens(input)
                .into_iter()
                .map(|t| (t.text, t.phonemes, t.whitespace))
                .collect();
            let tb: Tokens = theirs
                .tokens(input)
                .into_iter()
                .map(|t| (t.text, t.phonemes, t.whitespace))
                .collect();
            same_text += usize::from(a == b);
            same_tokens += usize::from(ta == tb);
            if (a != b || ta != tb) && listed < SHOWN {
                listed += 1;
                writeln!(shown, "- {name} `{input:?}`: ours `{a:?}`, loqui `{b:?}`")?;
                if a == b {
                    writeln!(shown, "  - tokens: ours `{ta:?}`, loqui `{tb:?}`")?;
                }
            }
        }
        let whole = started.elapsed();
        writeln!(
            rows,
            "| {name} | {} | {same_text} | {same_tokens} | {:.2} s / {:.2} s | {:.1} ms / {:.1} ms | {:.1} s |",
            inputs.len(),
            our_load.as_secs_f64(),
            ref_load.as_secs_f64(),
            our_prose.as_secs_f64() * 1e3,
            ref_prose.as_secs_f64() * 1e3,
            whole.as_secs_f64(),
        )?;
        let differ = inputs.len() * 2 - same_text - same_tokens;
        if differ > 0 {
            failed.push(format!("G2P {name}: {differ} result(s) differ from loqui"));
        }
    }
    writeln!(
        report,
        "| dialect | inputs | phonemize same | tokens same | load ours / loqui | \
         six openings ours / loqui | whole corpus, both |"
    )?;
    writeln!(report, "|---|---|---|---|---|---|---|")?;
    report.push_str(&rows);
    if !shown.is_empty() {
        writeln!(report, "\nFirst differences:\n\n{shown}")?;
    }
    writeln!(report)?;
    Ok(())
}
