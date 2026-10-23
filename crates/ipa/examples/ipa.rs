//! Lines on stdin as Kokoro reads them: `ipa es` or `ipa it`.

use std::io::{BufRead, Write};

use mareader_ipa::{Language, kokoro};

fn main() {
    let tag = std::env::args().nth(1).unwrap_or_default();
    let Some(language) = Language::ALL.into_iter().find(|l| l.tag() == tag) else {
        eprintln!("usage: ipa es|it < lines");
        std::process::exit(2);
    };
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let _ = writeln!(out, "{}", kokoro(language, &line));
    }
}
