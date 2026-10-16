//! Where the word levels and the tagger come from, and their files.

/// The repository both files live in, at its main branch.
const REPO: &str = "codewiththiha/cefr-rs";

/// One downloadable file: its path in the repository and its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: &'static str,
    /// What the download weighs, until the server says.
    pub bytes: u64,
    /// The name it is kept under once whole.
    pub file: &'static str,
}

/// About 248 000 senses, each word with its level per part of speech.
pub const DATASET: Source = Source {
    path: "data/cefr.zstd.parquet",
    bytes: 2_939_324,
    file: "cefr.parquet",
};

/// The model that reads a word's role in its sentence.
pub const TAGGER: Source = Source {
    path: "models/en_tokenizer.bin.zst",
    bytes: 6_696_057,
    file: "en_tokenizer.bin.zst",
};

impl Source {
    /// GitHub first, then two CDN mirrors of it.
    pub fn urls(&self) -> Vec<String> {
        let path = self.path;
        vec![
            format!("https://raw.githubusercontent.com/{REPO}/main/{path}"),
            format!("https://cdn.jsdelivr.net/gh/{REPO}@main/{path}"),
            format!("https://fastly.jsdelivr.net/gh/{REPO}@main/{path}"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_file_has_three_addresses() {
        let urls = TAGGER.urls();
        assert_eq!(urls.len(), 3);
        assert!(
            urls.iter()
                .all(|u| u.ends_with("/models/en_tokenizer.bin.zst"))
        );
        assert!(DATASET.urls()[0].starts_with("https://raw.githubusercontent.com/"));
    }
}
