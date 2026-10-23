use super::*;
use crate::tagger::tests::encode;
use crate::tokenize::tests::rules_json;

/// A phonemizer over a handful of words, spelling what it lacks.
fn small() -> Option<G2p> {
    let gold = serde_json::json!({
        "hello": "həlˈO", "world": "wˈɜɹld", "speak": "spˈik",
        "forty": "fˈɔɹɾi", "two": "tˈu", "five": "fˈIv", "dollar": "dˈɑləɹ",
        "nineteen": "nˌInˈitin", "oh": "ˈO", "twenty": "twˈɛnti", "first": "fˈɜɹst",
        "Z": "zˈi", "X": "ˈɛks",
    })
    .to_string();
    let tagger = encode(
        &["NN", "$", "."],
        &[("$", 1), (".", 2)],
        &[("bias", &[(0, 1.0)])],
    );
    let sources = Sources {
        tokenizer: &rules_json(),
        tagger: &tagger,
        gold: &gold,
        silver: "{}",
        neural: None,
    };
    G2p::from_sources(Dialect::American, sources).ok()
}

fn say(text: &str) -> String {
    small().map(|g| g.phonemize(text)).unwrap_or_default()
}

#[test]
fn a_phonemizer_is_shared_across_threads() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<G2p>();
}

#[test]
fn the_dialect_follows_the_voice_prefix() {
    assert_eq!(
        Dialect::for_kokoro_voice("af_heart"),
        Some(Dialect::American)
    );
    assert_eq!(
        Dialect::for_kokoro_voice("bm_george"),
        Some(Dialect::British)
    );
    assert_eq!(Dialect::for_kokoro_voice("jf_alpha"), None);
    assert_eq!(Dialect::for_kokoro_voice(""), None);
}

#[test]
fn words_come_from_the_lexicon_with_punctuation_kept() {
    assert_eq!(say("Hello world."), "həlˈO wˈɜɹld.");
    assert_eq!(say("[Kokoro](/kˈOkəɹO/) speaks."), "kˈOkəɹO spˈiks.");
}

#[test]
fn numbers_read_as_misaki_reads_them() {
    assert_eq!(say("42"), "fˈɔɹTi tˈu");
    assert_eq!(say("1905"), "nˌInˈitin ˈO fˈIv");
    assert_eq!(say("21st"), "twˈɛnti fˈɜɹst");
    assert_eq!(say("$5"), "fˈIv dˈɑləɹz");
}

#[test]
fn unknown_words_are_spelled_or_marked() {
    assert_eq!(say("zx"), "zˌiˈɛks");
    assert_eq!(say("日本"), UNKNOWN);
    let tokens = small().map(|g| g.tokens("日本 $5")).unwrap_or_default();
    let shown: Vec<_> = tokens
        .iter()
        .map(|t| (t.text.as_str(), t.phonemes.as_str(), t.whitespace))
        .collect();
    assert_eq!(
        shown,
        [
            ("日本", "", true),
            ("$", "", false),
            ("5", "fˈIv dˈɑləɹz", false)
        ]
    );
}

#[test]
fn missing_or_damaged_data_is_an_error() {
    let dir = std::env::temp_dir().join("mareader-g2p-missing");
    assert!(matches!(
        G2p::load(&dir, Dialect::British),
        Err(Error::Read { .. })
    ));
    let sources = Sources {
        tokenizer: "{}",
        tagger: &[],
        gold: "{}",
        silver: "{}",
        neural: None,
    };
    assert!(matches!(
        G2p::from_sources(Dialect::American, sources),
        Err(Error::Data(_))
    ));
}
