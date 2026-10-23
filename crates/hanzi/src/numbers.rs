//! Arabic digits to Chinese numerals, as cn2an's `an2cn` transform writes.

const DIGITS: [char; 10] = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

/// Place names, from units up; 16 places at most.
const PLACES: [&str; 16] = [
    "", "十", "百", "千", "万", "十", "百", "千", "亿", "十", "百", "千", "万", "十", "百", "千",
];

/// Measure words a range must come before to read as `到`.
const MEASURES: [&str; 34] = [
    "斤", "克", "千克", "公斤", "吨", "米", "厘米", "毫米", "公里", "升", "毫升", "元", "角", "分",
    "个", "只", "条", "张", "块", "瓶", "杯", "份", "本", "辆", "台", "匹", "头", "位", "亩",
    "小时", "分钟", "秒", "天", "半",
];

/// Every number in `text` in words; dates, fractions and percents too.
pub fn spell(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    if !chars.iter().any(|&c| digit(c).is_some()) {
        return text.into();
    }
    chars = pass(&chars, range, |m| {
        let text: String = m.iter().collect();
        let (from, to) = text.split_once('-')?;
        Some(format!("{}到{}", low(from)?, low(to)?))
    });
    chars = pass(&chars, date, |m| Some(date_words(m)));
    chars = pass(&chars, fraction, |m| {
        let text: String = m.iter().collect();
        let (top, bottom) = text.split_once('/')?;
        Some(format!("{}分之{}", low(bottom)?, low(top)?))
    });
    chars = pass(&chars, percent, |m| {
        let text: String = m.get(..m.len() - 1)?.iter().collect();
        Some(format!("百分之{}", low(&text)?))
    });
    chars = pass(&chars, celsius, |m| {
        let text: String = m.get(..m.len() - 1)?.iter().collect();
        Some(format!("{}摄氏度", low(&text)?))
    });
    chars = pass(&chars, number, |m| low(&m.iter().collect::<String>()));
    chars.into_iter().collect()
}

/// Replaces each match, leftmost first; a failed reading keeps the digits.
fn pass(
    chars: &[char],
    find: fn(&[char], usize) -> Option<usize>,
    say: impl Fn(&[char]) -> Option<String>,
) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        let found = find(chars, at).filter(|&end| end > at);
        match found.and_then(|end| Some((end, chars.get(at..end)?))) {
            Some((end, matched)) => {
                match say(matched) {
                    Some(words) => out.extend(words.chars()),
                    None => out.extend_from_slice(matched),
                }
                at = end;
            }
            None => {
                out.push(c);
                at += 1;
            }
        }
    }
    out
}

/// The digit's value, for ASCII and full-width digits.
fn digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        '０'..='９' => u8::try_from(u32::from(c) - u32::from('０')).ok(),
        _ => None,
    }
}

/// How many digits run from `at`.
fn digits(chars: &[char], at: usize) -> usize {
    let tail = chars.get(at..).unwrap_or_default();
    tail.iter().take_while(|&&c| digit(c).is_some()).count()
}

fn is(chars: &[char], at: usize, want: char) -> bool {
    chars.get(at) == Some(&want)
}

/// `\d+(\.\d+)?` from `at`: where it ends.
fn decimal(chars: &[char], at: usize) -> Option<usize> {
    let whole = digits(chars, at);
    if whole == 0 {
        return None;
    }
    let end = at + whole;
    match is(chars, end, '.') && digits(chars, end + 1) > 0 {
        true => Some(end + 1 + digits(chars, end + 1)),
        false => Some(end),
    }
}

/// `\d+(\.\d+)?-\d+(\.\d+)?` before a measure word.
fn range(chars: &[char], at: usize) -> Option<usize> {
    let mid = decimal(chars, at)?;
    if !is(chars, mid, '-') {
        return None;
    }
    let end = decimal(chars, mid + 1)?;
    let tail: String = chars.get(end..)?.iter().take(2).collect();
    MEASURES.iter().any(|m| tail.starts_with(m)).then_some(end)
}

/// `(\d{2,4}年)?(\d{1,2}月)?(\d{1,2}日)?`; may match nothing.
fn date(chars: &[char], at: usize) -> Option<usize> {
    let mut end = at;
    for (least, most, unit) in [(2, 4, '年'), (1, 2, '月'), (1, 2, '日')] {
        let run = digits(chars, end);
        if (least..=most).contains(&run) && is(chars, end + run, unit) {
            end += run + 1;
        }
    }
    Some(end)
}

/// The year digit by digit, the month and day as numbers.
fn date_words(matched: &[char]) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for &c in matched {
        if digit(c).is_some() {
            run.push(c);
            continue;
        }
        match c {
            '年' => out.extend(run.chars().filter_map(digit).map(spoken)),
            _ => out.push_str(&low(&run).unwrap_or_else(|| run.clone())),
        }
        run.clear();
        out.push(c);
    }
    out
}

fn fraction(chars: &[char], at: usize) -> Option<usize> {
    let top = digits(chars, at);
    let bottom = digits(chars, at + top + 1);
    (top > 0 && is(chars, at + top, '/') && bottom > 0).then_some(at + top + 1 + bottom)
}

/// `-?(\d+\.)?\d+`, as cn2an finds a number.
fn number(chars: &[char], at: usize) -> Option<usize> {
    let start = at + usize::from(is(chars, at, '-'));
    let whole = digits(chars, start);
    if whole == 0 {
        return None;
    }
    let point = start + whole;
    match is(chars, point, '.') && digits(chars, point + 1) > 0 {
        true => Some(point + 1 + digits(chars, point + 1)),
        false => Some(point),
    }
}

fn percent(chars: &[char], at: usize) -> Option<usize> {
    let end = number(chars, at)?;
    is(chars, end, '%').then_some(end + 1)
}

fn celsius(chars: &[char], at: usize) -> Option<usize> {
    let run = digits(chars, at);
    (run > 0 && is(chars, at + run, '℃')).then_some(at + run + 1)
}

fn spoken(d: u8) -> char {
    DIGITS.get(usize::from(d)).copied().unwrap_or('零')
}

/// A number in everyday numerals, as `一百二十三点五`; None past 16 places.
fn low(text: &str) -> Option<String> {
    let (sign, text) = match text.strip_prefix('-') {
        Some(rest) => ("负", rest),
        None => ("", text),
    };
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) if !fraction.contains('.') => (whole, Some(fraction)),
        Some(_) => return None,
        None => (text, None),
    };
    let whole: Vec<u8> = whole.chars().map(digit).collect::<Option<_>>()?;
    let mut out = format!("{sign}{}", integer(&whole)?);
    if let Some(fraction) = fraction {
        let fraction: Vec<u8> = fraction.chars().map(digit).collect::<Option<_>>()?;
        if !fraction.is_empty() {
            out.push('点');
        }
        out.extend(fraction.into_iter().take(16).map(spoken));
    }
    Some(out)
}

/// cn2an's integer reading, its zero rules and tidying kept as they are.
fn integer(digits: &[u8]) -> Option<String> {
    if digits.is_empty() {
        return None;
    }
    let first = digits
        .iter()
        .position(|&d| d != 0)
        .unwrap_or(digits.len() - 1);
    let digits = digits.get(first..)?;
    let len = digits.len();
    if len > PLACES.len() {
        return None;
    }
    let mut out = String::new();
    for (i, &d) in digits.iter().enumerate() {
        let place = PLACES.get(len - i - 1).copied().unwrap_or("");
        if d != 0 {
            out.push(spoken(d));
            out.push_str(place);
            continue;
        }
        if (len - i - 1) % 4 == 0 {
            out.push('零');
            out.push_str(place);
        }
        if i > 0 && !out.ends_with('零') {
            out.push('零');
        }
    }
    let out = out
        .replace("零零", "零")
        .replace("零万", "万")
        .replace("零亿", "亿")
        .replace("亿万", "亿");
    let out = drop_zero_before_thousands(out.trim_matches('零'));
    let out = match out.strip_prefix("一十") {
        Some(rest) => format!("十{rest}"),
        None => out,
    };
    Some(match out.is_empty() {
        true => "零".into(),
        false => out,
    })
}

/// `万零三千` to `万三千`, as cn2an tidies it.
fn drop_zero_before_thousands(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        out.push(c);
        let zero = chars.get(at + 1) == Some(&'零');
        let next = chars
            .get(at + 2)
            .is_some_and(|&d| "一二三四五六七八九壹贰叁肆伍陆柒捌玖".contains(d));
        let thousand = chars.get(at + 3).is_some_and(|&t| t == '千' || t == '仟');
        at += match matches!(c, '万' | '亿') && zero && next && thousand {
            true => 2,
            false => 1,
        };
    }
    out
}
