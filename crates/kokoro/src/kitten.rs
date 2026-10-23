//! Kitten's ways: its phoneme alphabet, padding, and voices file.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use candle_core::{Device, Tensor};

use crate::{Error, Result, Voice};

/// Kitten's symbols in id order; a repeated one keeps its last id.
const SYMBOLS: &str = concat!(
    "$",
    ";:,.!?¡¿—…\"«»\"\" ",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
    "ɑɐɒæɓʙβɔɕçɗɖðʤəɘɚɛɜɝɞɟʄɡɠɢʛɦɧħɥʜɨɪʝɭɬɫɮʟɱɯɰŋɳɲɴøɵɸθœɶʘɹɺɾɻʀʁɽʂʃʈʧʉʊʋⱱʌɣɤʍχʎʏʑʐʒʔʡʕʢǀǁǂǃˈˌːˑʼʴʰʱʲʷˠˤ˞↓↑→↗↘'\u{329}'ᵻ",
);

/// `…`, which every run ends with before the closing pad.
const TAIL: u32 = 10;

/// Samples cut from the end of each run, the tail `…` leaves.
pub const TRIM: usize = 5_000;

fn table() -> &'static HashMap<char, u32> {
    static TABLE: OnceLock<HashMap<char, u32>> = OnceLock::new();
    TABLE.get_or_init(|| (0u32..).zip(SYMBOLS.chars()).map(|(i, c)| (c, i)).collect())
}

/// The alphabet as a model config holds it.
pub(crate) fn vocab() -> HashMap<String, u32> {
    table().iter().map(|(c, &i)| (c.to_string(), i)).collect()
}

/// Words and marks apart with one space between, as Kitten learnt them.
pub fn spaced(phonemes: &str) -> String {
    let mut out = String::with_capacity(phonemes.len() + 8);
    let mut in_word = false;
    for c in phonemes.chars() {
        if c.is_whitespace() {
            in_word = false;
            continue;
        }
        let word = c.is_alphanumeric() || c == '_';
        if !out.is_empty() && !(word && in_word) {
            out.push(' ');
        }
        out.push(c);
        in_word = word;
    }
    out
}

/// Ids for espeak-style phonemes, padded as Kitten expects;
/// symbols it doesn't know are left out.
pub fn ids(phonemes: &str) -> Vec<u32> {
    let table = table();
    let mut ids = vec![0];
    ids.extend(
        spaced(phonemes)
            .chars()
            .filter_map(|c| table.get(&c).copied()),
    );
    ids.extend([TAIL, 0]);
    ids
}

/// The style row Kitten picks: by the text's length in characters.
pub fn style(voice: &Voice, text: &str) -> Result<Tensor> {
    voice.row(text.chars().count())
}

/// Every voice in a `voices.npz`, by name, sorted.
pub fn read_voices(path: &Path) -> Result<Vec<(String, Voice)>> {
    let bad = |what: &dyn std::fmt::Display| Error::Load(format!("{}: {what}", path.display()));
    let file = std::fs::File::open(path).map_err(|e| bad(&e))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| bad(&e))?;
    let mut voices = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| bad(&e))?;
        let name = entry.name().map_err(|e| bad(&e))?;
        let Some(name) = name.strip_suffix(".npy").map(str::to_owned) else {
            continue;
        };
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| bad(&e))?;
        let rows = npy(&bytes).map_err(|e| bad(&format!("{name}: {e}")))?;
        voices.push((name, Voice::from_tensor(rows)?));
    }
    if voices.is_empty() {
        return Err(bad(&"no voices inside"));
    }
    voices.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(voices)
}

/// A little-endian `f32` NumPy array, in C order.
fn npy(bytes: &[u8]) -> std::result::Result<Tensor, String> {
    let rest = bytes
        .strip_prefix(b"\x93NUMPY")
        .ok_or("not a NumPy array")?;
    let (&[major, _minor], rest) = rest.split_first_chunk::<2>().ok_or("cut short")?;
    let (len, rest) = match major {
        1 => {
            let (n, rest) = rest.split_first_chunk::<2>().ok_or("cut short")?;
            (usize::from(u16::from_le_bytes(*n)), rest)
        }
        _ => {
            let (n, rest) = rest.split_first_chunk::<4>().ok_or("cut short")?;
            let n = usize::try_from(u32::from_le_bytes(*n)).map_err(|_| "too long")?;
            (n, rest)
        }
    };
    let header = rest.get(..len).ok_or("cut short")?;
    let header = std::str::from_utf8(header).map_err(|_| "a header that isn't text")?;
    if !header.contains("'<f4'") || header.contains("'fortran_order': True") {
        return Err(format!("not little-endian f32 in C order: {header}"));
    }
    let shape = header
        .split_once("'shape': (")
        .and_then(|(_, s)| s.split_once(')'))
        .map(|(s, _)| s)
        .ok_or("no shape")?;
    let dims = shape
        .split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            d.parse::<usize>()
                .map_err(|_| format!("a shape of {shape}"))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let data = rest.get(len..).ok_or("cut short")?;
    let (words, extra) = data.as_chunks::<4>();
    let count: usize = dims.iter().product();
    if words.len() != count || !extra.is_empty() {
        return Err(format!("{} values for a shape of {shape}", words.len()));
    }
    let values = words.iter().map(|&w| f32::from_le_bytes(w)).collect();
    Tensor::from_vec(values, dims, &Device::Cpu).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_alphabet_matches_kitten_ids() {
        let t = table();
        assert_eq!(t.len(), 175);
        let id = |c: char| t.get(&c).copied();
        assert_eq!(id('$'), Some(0));
        assert_eq!(id('…'), Some(TAIL));
        assert_eq!(id('"'), Some(15), "the last of its three");
        assert_eq!(id(' '), Some(16));
        assert_eq!(id('A'), Some(17));
        assert_eq!(id('ə'), Some(83));
        assert_eq!(id('ˈ'), Some(156));
        assert_eq!(id('\''), Some(176));
        assert_eq!(id('ᵻ'), Some(177));
    }

    #[test]
    fn marks_stand_apart_and_runs_are_padded() {
        assert_eq!(spaced("həlˈoʊ,  wˈɜːld!"), "həlˈoʊ , wˈɜːld !");
        assert_eq!(spaced(" \"aɪ\" "), "\" aɪ \"");
        assert_eq!(ids("a."), [0, 43, 16, 4, TAIL, 0]);
        // As upstream: the unknown mark is spaced apart, then dropped.
        assert_eq!(ids("a\u{1F600}"), [0, 43, 16, TAIL, 0]);
    }

    fn npy_bytes(header: &str, values: &[f32]) -> Vec<u8> {
        let mut b = b"\x93NUMPY\x01\x00".to_vec();
        b.extend_from_slice(&(header.len() as u16).to_le_bytes());
        b.extend_from_slice(header.as_bytes());
        b.extend(values.iter().flat_map(|v| v.to_le_bytes()));
        b
    }

    #[test]
    fn arrays_read_with_their_shape() {
        let h = "{'descr': '<f4', 'fortran_order': False, 'shape': (2, 3), }";
        let t = npy(&npy_bytes(h, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])).unwrap();
        assert_eq!(t.dims(), [2, 3]);
        assert!(npy(&npy_bytes(h, &[1.0; 5])).is_err());
        let wrong = "{'descr': '<f8', 'fortran_order': False, 'shape': (1,), }";
        assert!(npy(&npy_bytes(wrong, &[1.0, 2.0])).is_err());
        assert!(npy(b"PK\x03\x04").is_err());
    }
}
