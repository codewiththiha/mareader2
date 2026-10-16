//! The packs on offer: which parquet joins which two languages.

const REPO: &str = "https://raw.githubusercontent.com/codewiththiha/wikidict/main/output/curated";
const CDN: &str = "https://cdn.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated";
const FASTLY: &str = "https://fastly.jsdelivr.net/gh/codewiththiha/wikidict@main/output/curated";

/// One downloadable dictionary, as mareader offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pack {
    /// Its id and its files' stem.
    pub id: &'static str,
    /// The headword language; English in every pack so far.
    pub source: &'static str,
    /// The definition language.
    pub target: &'static str,
    pub entries: u64,
    /// The parquet's size when listed, for a size before the download.
    pub bytes: u64,
}

/// Every pack the app offers. A language is one more row.
pub const PACKS: [Pack; 3] = [
    Pack {
        id: "mcfnlp-en-my",
        source: "en",
        target: "my",
        entries: 110_640,
        bytes: 2_875_539,
    },
    Pack {
        id: "jmdict-en-jp",
        source: "en",
        target: "jp",
        entries: 441_348,
        bytes: 7_026_188,
    },
    Pack {
        id: "wiktionary-en-fr",
        source: "en",
        target: "fr",
        entries: 128_263,
        bytes: 3_218_737,
    },
];

impl Pack {
    /// The pack with this id, if it is offered.
    pub fn get(id: &str) -> Option<&'static Pack> {
        PACKS.iter().find(|p| p.id == id)
    }

    /// Where to fetch it: the repository, then two mirrors of it.
    pub fn urls(&self) -> [String; 3] {
        [REPO, CDN, FASTLY].map(|base| format!("{base}/{}.parquet", self.id))
    }

    /// "English → Burmese".
    pub fn label(&self) -> String {
        format!(
            "{} → {}",
            crate::lang::name(self.source),
            crate::lang::name(self.target)
        )
    }

    /// Whether it joins `a` and `b`, either way round.
    pub fn joins(&self, a: &str, b: &str) -> bool {
        (self.source == a && self.target == b) || (self.source == b && self.target == a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pack_is_fetched_from_three_places_by_its_id() {
        for pack in PACKS {
            let urls = pack.urls();
            assert!(
                urls.iter()
                    .all(|u| u.ends_with(&format!("/{}.parquet", pack.id)))
            );
            assert!(urls.iter().all(|u| u.starts_with("https://")));
            assert_eq!(Pack::get(pack.id), Some(&pack));
        }
        assert_eq!(Pack::get("nope"), None);
        assert_eq!(PACKS[0].label(), "English → Burmese");
    }
}
