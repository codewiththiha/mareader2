//! Checks for the Chinese reader on small made-up data.

use crate::pinyin::Pinyin;
use crate::table::Table;
use crate::words::Words;
use crate::{Hanzi, numbers, punctuate, syllable_ipa};

/// 北 běi, 京 jīng or jìng, 银 yín, 行 xíng or háng.
const CHARS: &str = r#"{"21271": "běi", "20140": "jīng,jìng",
    "38134": "yín", "34892": "xíng,háng"}"#;
const PHRASES: &str = r#"{"银行": [["yín"], ["háng"]], "北京": [["běi"], ["jīng"]]}"#;
const DICT: &str = "银行 50 n\n行 20 v\n北 10 f\n";
/// 北 likely begins a word and 京 ends one; nothing else is known.
const EMIT: &str = "P={'B': {'\\u5317': -1.0},\n 'E': {'\\u4eac': -1.0},\n \
                    'M': {'\\u4e00': -9.0},\n 'S': {'\\u884c': -2.0}}";

fn hanzi() -> Hanzi {
    Hanzi::parse(CHARS, PHRASES, DICT, EMIT).unwrap()
}

#[test]
fn numbers_read_as_cn2an_writes_them() {
    let cases = [
        ("3", "三"),
        ("10", "十"),
        ("100", "一百"),
        ("1001", "一千零一"),
        ("10010", "一万零一十"),
        ("100000", "十万"),
        ("1000000", "一百万"),
        ("20005", "二万零五"),
        ("12345678", "一千二百三十四万五千六百七十八"),
        ("100000000", "一亿"),
        ("110000000", "一亿一千万"),
        ("10000001", "一千万零一"),
        ("100100", "十万零一百"),
        ("-5", "负五"),
        ("3.14", "三点一四"),
        ("0.5", "零点五"),
        ("007", "七"),
        ("12345678901234567", "12345678901234567"),
        ("1/2", "二分之一"),
        ("50%", "百分之五十"),
        ("-1.5%", "百分之负一点五"),
        ("30℃", "三十摄氏度"),
        ("1-2个", "一到二个"),
        ("2026年10月23日", "二零二六年十月二十三日"),
        ("123月", "一二十三月"),
        ("100日", "一零日"),
        ("1.5.3%", "一.百分之五点三"),
        ("1,000", "一,零"),
        ("３个", "三个"),
        ("x-3", "x负三"),
    ];
    for (digits, words) in cases {
        assert_eq!(numbers::spell(digits), words, "{digits}");
    }
}

#[test]
fn syllables_read_as_misaki_hears_them() {
    let cases = [
        ("zhōng", "ꭧʊ→ŋ"),
        ("shì", "ʂɨ↘"),
        ("zi", "ʦɨ"),
        ("lüè", "lɥe↘"),
        ("nǚ", "ny↓"),
        ("jiǔ", "ʨjou↓"),
        ("guì", "kwei↘"),
        ("lún", "lwə↗n"),
        ("yuán", "ɥɛ↗n"),
        ("yǒng", "jʊ↓ŋ"),
        ("wēng", "wə→ŋ"),
        ("yō", "wo→"),
        ("wòng", "ʊ↘ŋ"),
        ("ń", "n↗"),
        ("hm", "hm"),
        ("er", "ɚ"),
        ("ê\u{304}", "ɛ→"),
        ("xyz", ""),
    ];
    for (pinyin, ipa) in cases {
        assert_eq!(syllable_ipa(pinyin), ipa, "{pinyin}");
    }
}

#[test]
fn punctuation_is_spaced_as_misaki_maps_it() {
    assert_eq!(punctuate("好，走。《书》（注）"), "好, 走.  “书”  (注)");
    assert_eq!(punctuate("他说：“你好吗？”"), "他说: “你好吗? ”");
}

#[test]
fn a_table_finds_words_and_prefixes() {
    let table = Table::new(vec![("中国", 1), ("中国人", 2), ("人", 3), ("中国", 4)]).unwrap();
    assert_eq!(table.get("中国"), Some(4));
    assert_eq!(table.get("中"), None);
    assert!(table.has_prefix("中"));
    assert!(table.has_prefix("中国人"));
    assert!(!table.has_prefix("国"));
    assert_eq!(table.len(), 3);
}

#[test]
fn known_words_take_the_likeliest_path() {
    let words = Words::parse(DICT, EMIT).unwrap();
    let mut out = Vec::new();
    words.cut("银行行", &mut out);
    assert_eq!(out, ["银行", "行"]);
}

#[test]
fn unknown_runs_are_guessed_by_the_model() {
    let words = Words::parse(DICT, EMIT).unwrap();
    let mut out = Vec::new();
    words.cut("北京", &mut out);
    assert_eq!(out, ["北京"]);
}

#[test]
fn phrases_override_the_first_reading() {
    let pinyin = Pinyin::parse(CHARS, PHRASES).unwrap();
    let mut out = String::new();
    pinyin.push("银行", &mut out);
    assert_eq!(out, "i↗nxa↗ŋ");
    out.clear();
    // The longest phrase first, then the rest by character.
    pinyin.push("银行行", &mut out);
    assert_eq!(out, "i↗nxa↗ŋɕi↗ŋ");
}

#[test]
fn text_reads_with_words_apart() {
    let hanzi = hanzi();
    assert_eq!(hanzi.size(), (4, 2, 3));
    assert_eq!(hanzi.phonemize("北京银行行。"), "pei↓ʨi→ŋ i↗nxa↗ŋ ɕi↗ŋ.");
    assert_eq!(hanzi.phonemize("OK，北京"), "OK, pei↓ʨi→ŋ");
    assert_eq!(hanzi.phonemize("  "), "");
}

#[test]
fn broken_files_are_errors() {
    assert!(Hanzi::parse("[]", PHRASES, DICT, EMIT).is_err());
    assert!(Hanzi::parse(CHARS, "{", DICT, EMIT).is_err());
    assert!(Hanzi::parse(CHARS, PHRASES, "银行 many\n", EMIT).is_err());
    assert!(Hanzi::parse(CHARS, PHRASES, DICT, "P={'X': {}}").is_err());
}
