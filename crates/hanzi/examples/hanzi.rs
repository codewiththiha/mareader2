//! Lines on stdin as Kokoro reads them: `hanzi DIR [--syllables]`.

use std::io::{BufRead, Write};

use mareader_hanzi::Hanzi;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_default();
    let syllables = args.next().as_deref() == Some("--syllables");
    let hanzi = match Hanzi::load(std::path::Path::new(&dir)) {
        Ok(hanzi) => hanzi,
        Err(e) => {
            eprintln!("usage: hanzi DIR [--syllables] < lines ({e})");
            std::process::exit(2);
        }
    };
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let said = match syllables {
            true => mareader_hanzi::syllable_ipa(&line),
            false => hanzi.phonemize(&line),
        };
        let _ = writeln!(out, "{said}");
    }
}
