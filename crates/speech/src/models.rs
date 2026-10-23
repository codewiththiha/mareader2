//! Voice models to download: their files, voices and where they come from.

/// One file a model needs, by its path inside the model's folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelFile {
    pub path: &'static str,
    pub bytes: u64,
    /// Lowercase hex, checked before the file is kept.
    pub sha256: &'static str,
    /// Its own addresses, when not the model's.
    pub hosts: Option<&'static [&'static str]>,
    /// Its name there, when not `path`.
    pub from: Option<&'static str>,
}

/// How a voice sounds to a listener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gender {
    Female,
    Male,
    Neutral,
}

impl Gender {
    pub fn label(self) -> &'static str {
        match self {
            Self::Female => "female",
            Self::Male => "male",
            Self::Neutral => "neutral",
        }
    }
}

/// A voice a model speaks in; each downloads on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelVoice {
    pub id: &'static str,
    pub name: &'static str,
    /// A BCP 47 tag, as `en-GB`.
    pub language: &'static str,
    pub gender: Gender,
    /// None when the voice comes inside every build.
    pub file: Option<ModelFile>,
    /// A small file the voice needs beside `file`, such as its settings.
    pub extra: Option<ModelFile>,
    /// What sets it apart, as `Slower to make`; empty if nothing.
    pub note: &'static str,
}

impl ModelVoice {
    /// Every file the voice downloads.
    pub fn files(&self) -> Vec<ModelFile> {
        self.file.into_iter().chain(self.extra).collect()
    }

    pub fn bytes(&self) -> u64 {
        self.files().iter().map(|f| f.bytes).sum()
    }
}

/// A setting a model offers, as a share of each voice's own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knob {
    /// Its key in settings.
    pub id: &'static str,
    pub name: &'static str,
    /// One line on what it changes.
    pub about: &'static str,
    /// Least and most; 1 is the voice as it was made.
    pub range: (f32, f32),
}

/// Ranges are listed numbers, never NaN, so each equals itself.
impl Eq for Knob {}

impl Knob {
    /// `value` within range; a value not set, or not a number, is 1.
    pub fn clamp(&self, value: Option<f32>) -> f32 {
        let value = value.filter(|v| v.is_finite()).unwrap_or(1.0);
        value.clamp(self.range.0, self.range.1)
    }
}

/// One way to get a model's weights: the voices are shared, the sizes differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Build {
    pub id: &'static str,
    pub name: &'static str,
    /// One line on what it trades.
    pub about: &'static str,
    pub file: ModelFile,
    /// A small file the build needs beside `file`, such as its voices.
    pub extra: Option<ModelFile>,
}

impl Build {
    /// Every file the build downloads.
    pub fn files(&self) -> Vec<ModelFile> {
        std::iter::once(self.file).chain(self.extra).collect()
    }

    pub fn bytes(&self) -> u64 {
        self.files().iter().map(|f| f.bytes).sum()
    }
}

/// A model on offer: the engine it becomes once downloaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    /// Its engine id and its folder's name.
    pub id: &'static str,
    pub name: &'static str,
    /// One line on what it is.
    pub about: &'static str,
    pub license: &'static str,
    /// Addresses a file's path is joined to, tried in turn.
    pub hosts: &'static [&'static str],
    /// What every build needs before any voice.
    pub files: &'static [ModelFile],
    /// Its weights in each size, best first; none when voices bring their own.
    pub builds: &'static [Build],
    pub voices: &'static [ModelVoice],
    /// Comes with the model, so it can speak at once.
    pub first_voice: &'static str,
    /// Settings that change how it sounds.
    pub knobs: &'static [Knob],
}

impl Model {
    /// The voice with this id, if it has one.
    pub fn voice(&self, id: &str) -> Option<&'static ModelVoice> {
        self.voices.iter().find(|v| v.id == id)
    }

    /// The voice the model downloads with.
    pub fn first(&self) -> Option<&'static ModelVoice> {
        self.voice(self.first_voice)
    }

    pub fn build(&self, id: &str) -> Option<&'static Build> {
        self.builds.iter().find(|b| b.id == id)
    }

    /// The files every build shares: its files and first voice.
    pub fn core(&self) -> Vec<ModelFile> {
        let mut files: Vec<ModelFile> = self.files.to_vec();
        // Small files first, so the big one ends the download.
        files.extend(self.first().iter().flat_map(|v| v.files()));
        files.sort_by_key(|f| f.bytes);
        files
    }

    /// What the shared files weigh.
    pub fn bytes(&self) -> u64 {
        self.core().iter().map(|f| f.bytes).sum()
    }

    /// What a first download with `build` weighs.
    pub fn bytes_with(&self, build: &Build) -> u64 {
        self.bytes() + build.bytes()
    }

    /// Every address `file` can be fetched from.
    pub fn urls(&self, file: &ModelFile) -> Vec<String> {
        let hosts = file.hosts.unwrap_or(self.hosts);
        let name = file.from.unwrap_or(file.path);
        hosts.iter().map(|h| format!("{h}{name}")).collect()
    }
}

/// Every model on offer.
pub const MODELS: &[Model] = &[KOKORO, PIPER, KITTEN];

/// Pinned, so a file never changes under its checksum.
const KOKORO_HOSTS: &[&str] = &[
    "https://huggingface.co/hexgrad/Kokoro-82M/resolve/f3ff3571791e39611d31c381e3a41a3af07b4987/",
    "https://hf-mirror.com/hexgrad/Kokoro-82M/resolve/f3ff3571791e39611d31c381e3a41a3af07b4987/",
];

/// The same weights converted to GGUF, Apache 2.0 as well; pinned.
const GGUF_HOSTS: &[&str] = &[
    "https://huggingface.co/cstr/kokoro-82m-GGUF/resolve/ab19534a3b5cc12bc2741478e9790eadb328654a/",
    "https://hf-mirror.com/cstr/kokoro-82m-GGUF/resolve/ab19534a3b5cc12bc2741478e9790eadb328654a/",
];

/// Pinned to a commit; the files are loqui-g2p 0.4.0's, byte for byte.
const G2P_HOSTS: &[&str] = &[
    "https://raw.githubusercontent.com/Awakened-Labs/loqui/717691aadf67408c85d87dc33e5d1d77b2ea4b4d/crates/loqui-g2p/data/",
    "https://cdn.jsdelivr.net/gh/Awakened-Labs/loqui@717691aadf67408c85d87dc33e5d1d77b2ea4b4d/crates/loqui-g2p/data/",
];

/// English pronunciation data, read by `mareader-g2p`.
const fn g2p(path: &'static str, bytes: u64, sha256: &'static str) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256,
        hosts: Some(G2P_HOSTS),
        from: None,
    }
}

/// Every file `mareader-g2p` reads, for both dialects.
const G2P: [ModelFile; 8] = [
    g2p(
        "spacy-en-tokenizer.json",
        79_554,
        "2ffa6b3b6f4da114531ff897c02f3d2551485ef45c3ca1f6e6b2fdc80e034d1d",
    ),
    g2p(
        "tagger-en.bin",
        1_415_442,
        "df3d6cb2c34d7da57f92e47ae9dea6bcc9c9f8ed7cf9ceec703e84d7d67d698f",
    ),
    g2p(
        "misaki-us_gold.json",
        3_000_469,
        "dc414872a49a28ae6c141463d502fd945f3b2fde040484fdc47d00cc4612686f",
    ),
    g2p(
        "misaki-us_silver.json",
        3_099_517,
        "de8f67be911bb6c659187b4a65fd966b6a30e56350e0f790d763210b053ac475",
    ),
    g2p(
        "g2p-en-us.safetensors",
        3_011_692,
        "dc4a02e62d4fcb4bb4097ecf00db89b8e1a12a549a52ab6adfbba220b80a55c5",
    ),
    g2p(
        "misaki-gb_gold.json",
        2_838_552,
        "29e62f4b60261c88f7f3c2c7811ca3825978948090b72d2b27d565b729282f71",
    ),
    g2p(
        "misaki-gb_silver.json",
        3_663_898,
        "48131e2d92ccc41655f4543e87e0f938e71463eb5a54be7f0693bb712ebb6bce",
    ),
    g2p(
        "g2p-en-gb.safetensors",
        3_011_692,
        "4994f474bb6f4584076a4e98189caaec11aa3f773a0d82d5bc263a03dd07e703",
    ),
];

/// Natural speech from a small model that runs on the computer.
pub const KOKORO: Model = Model {
    id: "kokoro",
    name: "Kokoro",
    about: "Natural voices in English, Spanish, Italian, Portuguese, French and Hindi from a small model, 82 million parameters",
    license: "Apache 2.0",
    hosts: KOKORO_HOSTS,
    files: &[
        ModelFile {
            path: "config.json",
            bytes: 2_351,
            sha256: "5abb01e2403b072bf03d04fde160443e209d7a0dad49a423be15196b9b43c17f",
            hosts: None,
            from: None,
        },
        G2P[0],
        G2P[1],
        G2P[2],
        G2P[3],
        G2P[4],
        G2P[5],
        G2P[6],
        G2P[7],
    ],
    builds: &[
        Build {
            id: "full",
            name: "Full",
            about: "The original weights: the best sound",
            file: ModelFile {
                path: "kokoro-v1_0.pth",
                bytes: 327_212_226,
                sha256: "496dba118d1a58f5f3db2efc88dbdc216e0483fc89fe6e47ee1f2c53f18ad1e4",
                hosts: None,
                from: None,
            },
            extra: None,
        },
        Build {
            id: "half",
            name: "Half",
            about: "Half-precision weights: half the download, near the same sound",
            file: ModelFile {
                path: "kokoro-82m-f16.gguf",
                bytes: 163_728_096,
                sha256: "341d5e96c2d7f06fc71dbc8d955150a5b6e3a59840f8905cb3fbe9b12120df52",
                hosts: Some(GGUF_HOSTS),
                from: None,
            },
            extra: None,
        },
        Build {
            id: "small",
            name: "Small",
            about: "8-bit weights: the smallest download, a little rougher",
            file: ModelFile {
                path: "kokoro-82m-q8_0.gguf",
                bytes: 141_322_336,
                sha256: "3c8b60064b287a10abd35396a59c9981675c618db8bc91bd290db63f55eb492d",
                hosts: Some(GGUF_HOSTS),
                from: None,
            },
            extra: None,
        },
    ],
    voices: KOKORO_VOICES,
    first_voice: "af_heart",
    knobs: &[],
};

/// A Kokoro voice; `path` is `voices/<id>.pt`.
const fn voice(
    (id, name, language): (&'static str, &'static str, &'static str),
    feminine: bool,
    path: &'static str,
    (bytes, sha256): (u64, &'static str),
) -> ModelVoice {
    ModelVoice {
        id,
        name,
        language,
        gender: match feminine {
            true => Gender::Female,
            false => Gender::Male,
        },
        file: Some(ModelFile {
            path,
            bytes,
            sha256,
            hosts: None,
            from: None,
        }),
        extra: None,
        note: "",
    }
}

/// Ids: language `a`, `b`, `e`, `i`, `p`, `f` or `h`, then `f` or `m`.
const KOKORO_VOICES: &[ModelVoice] = &[
    voice(
        ("af_alloy", "Alloy", "en-US"),
        true,
        "voices/af_alloy.pt",
        (
            523_425,
            "6d877149dd8b348fbad12e5845b7e43d975390e9f3b68a811d1d86168bef5aa3",
        ),
    ),
    voice(
        ("af_aoede", "Aoede", "en-US"),
        true,
        "voices/af_aoede.pt",
        (
            523_425,
            "c03bd1a4c3716c2d8eaa3d50022f62d5c31cfbd6e15933a00b17fefe13841cc4",
        ),
    ),
    voice(
        ("af_bella", "Bella", "en-US"),
        true,
        "voices/af_bella.pt",
        (
            523_425,
            "8cb64e02fcc8de0327a8e13817e49c76c945ecf0052ceac97d3081480e8e48d6",
        ),
    ),
    voice(
        ("af_heart", "Heart", "en-US"),
        true,
        "voices/af_heart.pt",
        (
            523_425,
            "0ab5709b8ffab19bfd849cd11d98f75b60af7733253ad0d67b12382a102cb4ff",
        ),
    ),
    voice(
        ("af_jessica", "Jessica", "en-US"),
        true,
        "voices/af_jessica.pt",
        (
            523_435,
            "cdfdccb8cc975aa34ee6b89642963b0064237675de0e41a30ae64cc958dd4e87",
        ),
    ),
    voice(
        ("af_kore", "Kore", "en-US"),
        true,
        "voices/af_kore.pt",
        (
            523_420,
            "8bfbc512321c3db49dff984ac675fa5ac7eaed5a96cc31104d3a9080e179d69d",
        ),
    ),
    voice(
        ("af_nicole", "Nicole", "en-US"),
        true,
        "voices/af_nicole.pt",
        (
            523_430,
            "c5561808bcf5250fe8c5f5de32caf2d94f27e57e95befdb098c5c85991d4c5da",
        ),
    ),
    voice(
        ("af_nova", "Nova", "en-US"),
        true,
        "voices/af_nova.pt",
        (
            523_420,
            "e0233676ddc21908c37a1f102f6b88a59e4e5c1bd764983616eb9eda629dbcd2",
        ),
    ),
    voice(
        ("af_river", "River", "en-US"),
        true,
        "voices/af_river.pt",
        (
            523_425,
            "e149459bd9c084416b74756b9bd3418256a8b839088abb07d463730c369dab8f",
        ),
    ),
    voice(
        ("af_sarah", "Sarah", "en-US"),
        true,
        "voices/af_sarah.pt",
        (
            523_425,
            "49bd364ea3be9eb3e9685e8f9a15448c4883112a7c0ff7ab139fa4088b08cef9",
        ),
    ),
    voice(
        ("af_sky", "Sky", "en-US"),
        true,
        "voices/af_sky.pt",
        (
            523_351,
            "c799548aed06e0cb0d655a85a01b48e7f10484d71663f9a3045a5b9362e8512c",
        ),
    ),
    voice(
        ("am_adam", "Adam", "en-US"),
        false,
        "voices/am_adam.pt",
        (
            523_420,
            "ced7e284aba12472891be1da3ab34db84cc05cc02b5889535796dbf2d8b0cb34",
        ),
    ),
    voice(
        ("am_echo", "Echo", "en-US"),
        false,
        "voices/am_echo.pt",
        (
            523_420,
            "8bcfdc852bc985fb45c396c561e571ffb9183930071f962f1b50df5c97b161e8",
        ),
    ),
    voice(
        ("am_eric", "Eric", "en-US"),
        false,
        "voices/am_eric.pt",
        (
            523_420,
            "ada66f0eefff34ec921b1d7474d7ac8bec00cd863c170f1c534916e9b8212aae",
        ),
    ),
    voice(
        ("am_fenrir", "Fenrir", "en-US"),
        false,
        "voices/am_fenrir.pt",
        (
            523_430,
            "98e507eca1db08230ae3b6232d59c10aec9630022d19accac4f5d12fcec3c37a",
        ),
    ),
    voice(
        ("am_liam", "Liam", "en-US"),
        false,
        "voices/am_liam.pt",
        (
            523_420,
            "c82550757ddb31308b97f30040dda8c2d609a9e2de6135848d0a948368138518",
        ),
    ),
    voice(
        ("am_michael", "Michael", "en-US"),
        false,
        "voices/am_michael.pt",
        (
            523_435,
            "9a443b79a4b22489a5b0ab7c651a0bcd1a30bef675c28333f06971abbd47bd37",
        ),
    ),
    voice(
        ("am_onyx", "Onyx", "en-US"),
        false,
        "voices/am_onyx.pt",
        (
            523_420,
            "e8452be16cd0f6da7b4579eaf7b1e4506e92524882053d86d72b96b9a7fed584",
        ),
    ),
    voice(
        ("am_puck", "Puck", "en-US"),
        false,
        "voices/am_puck.pt",
        (
            523_420,
            "dd1d8973f4ce4b7d8ae407c77a435f485dabc052081b80ea75c4f30b84f36223",
        ),
    ),
    voice(
        ("am_santa", "Santa", "en-US"),
        false,
        "voices/am_santa.pt",
        (
            523_425,
            "7f2f7582fa2b1f160e90aafe6d0b442a685e773608b6667e545d743b073e97a7",
        ),
    ),
    voice(
        ("bf_alice", "Alice", "en-GB"),
        true,
        "voices/bf_alice.pt",
        (
            523_425,
            "d292651b6af6c0d81705c2580dcb4463fccc0ff7b8d618a471dbb4e45655b3f3",
        ),
    ),
    voice(
        ("bf_emma", "Emma", "en-GB"),
        true,
        "voices/bf_emma.pt",
        (
            523_420,
            "d0a423deabf4a52b4f49318c51742c54e21bb89bbbe9a12141e7758ddb5da701",
        ),
    ),
    voice(
        ("bf_isabella", "Isabella", "en-GB"),
        true,
        "voices/bf_isabella.pt",
        (
            523_440,
            "cdd4c37003805104d1d08fb1e05855c8fb2c68de24ca6e71f264a30aaa59eefd",
        ),
    ),
    voice(
        ("bf_lily", "Lily", "en-GB"),
        true,
        "voices/bf_lily.pt",
        (
            523_420,
            "6e09c2e481e2d53004d7e5ae7d3a325369e130a6f45c35a6002de75084be9285",
        ),
    ),
    voice(
        ("bm_daniel", "Daniel", "en-GB"),
        false,
        "voices/bm_daniel.pt",
        (
            523_430,
            "fc3fce4e9c12ed4dbc8fa9680cfe51ee190a96444ce7c3ad647549a30823fc5d",
        ),
    ),
    voice(
        ("bm_fable", "Fable", "en-GB"),
        false,
        "voices/bm_fable.pt",
        (
            523_425,
            "d44935f3135257a9064df99f007fc1342ff1aa767552b4a4fa4c3b2e6e59079c",
        ),
    ),
    voice(
        ("bm_george", "George", "en-GB"),
        false,
        "voices/bm_george.pt",
        (
            523_430,
            "f1bc812213dc59774769e5c80004b13eeb79bd78130b11b2d7f934542dab811b",
        ),
    ),
    voice(
        ("bm_lewis", "Lewis", "en-GB"),
        false,
        "voices/bm_lewis.pt",
        (
            523_425,
            "b5204750dcba01029d2ac9cec17aec3b20a6d64073c579d694a23cb40effbd0e",
        ),
    ),
    voice(
        ("ef_dora", "Dora", "es-ES"),
        true,
        "voices/ef_dora.pt",
        (
            523_420,
            "d9d69b0f8a2b87a345f269d89639f89dfbd1a6c9da0c498ae36dd34afcf35530",
        ),
    ),
    voice(
        ("em_alex", "Alex", "es-ES"),
        false,
        "voices/em_alex.pt",
        (
            523_420,
            "5eac53f767c3f31a081918ba531969aea850bed18fe56419b804d642c6973431",
        ),
    ),
    voice(
        ("em_santa", "Santa", "es-ES"),
        false,
        "voices/em_santa.pt",
        (
            523_430,
            "aa8620cb96cec705823efca0d956a63e158e09ad41aca934d354b7f0778f63cb",
        ),
    ),
    voice(
        ("if_sara", "Sara", "it-IT"),
        true,
        "voices/if_sara.pt",
        (
            523_425,
            "6c0b253b955fe32f1a1a86006aebe83d050ea95afd0e7be15182f087deedbf55",
        ),
    ),
    voice(
        ("im_nicola", "Nicola", "it-IT"),
        false,
        "voices/im_nicola.pt",
        (
            523_341,
            "234ed06648649f9bd874b37508ea17560b9c993ef85b4ddb3e3a71e062bd2c12",
        ),
    ),
    voice(
        ("pf_dora", "Dora", "pt-BR"),
        true,
        "voices/pf_dora.pt",
        (
            523_425,
            "07e4ff987c5d5a8c3995efd15cc4f0db7c4c15e881b198d8ab7f67ecf51f5eb7",
        ),
    ),
    voice(
        ("pm_alex", "Alex", "pt-BR"),
        false,
        "voices/pm_alex.pt",
        (
            523_425,
            "cf0ba8c573c2480fc54123683a35cf1e2ae130428e441eb91f9149bdb188a526",
        ),
    ),
    voice(
        ("pm_santa", "Santa", "pt-BR"),
        false,
        "voices/pm_santa.pt",
        (
            523_430,
            "d42103169c5c872abbafb9129133af7e942bb9d272c3cc3b95c203e7d7198c29",
        ),
    ),
    voice(
        ("ff_siwis", "Siwis", "fr-FR"),
        true,
        "voices/ff_siwis.pt",
        (
            523_425,
            "8073bf2d2c4b9543a90f2f0fd2144de4ed157e2d4b79ddeb0d5123066171fbc9",
        ),
    ),
    voice(
        ("hf_alpha", "Alpha", "hi-IN"),
        true,
        "voices/hf_alpha.pt",
        (
            523_425,
            "06906fe05746d13a79c5c01e21fd7233b05027221a933c9ada650f5aafc8f044",
        ),
    ),
    voice(
        ("hf_beta", "Beta", "hi-IN"),
        true,
        "voices/hf_beta.pt",
        (
            523_420,
            "63c0a1a6272e98d43f4511bba40e30dd9c8ceaf5f39af869509b9f51a71c503e",
        ),
    ),
    voice(
        ("hm_omega", "Omega", "hi-IN"),
        false,
        "voices/hm_omega.pt",
        (
            523_425,
            "b55f02a8e8483fffe0afa566e7d22ed8013acf47ad4f6bbee2795a840155703e",
        ),
    ),
    voice(
        ("hm_psi", "Psi", "hi-IN"),
        false,
        "voices/hm_psi.pt",
        (
            523_351,
            "2f0f055cea4f1083f4ef127ece48d71606347f6557dbb961c0ca5740a2da485b",
        ),
    ),
];

/// Pinned, so a file never changes under its checksum.
const PIPER_HOSTS: &[&str] = &[
    "https://huggingface.co/rhasspy/piper-voices/resolve/5041a7a49ae5b3a0f30e6837769eb03fdbb231bd/",
    "https://hf-mirror.com/rhasspy/piper-voices/resolve/5041a7a49ae5b3a0f30e6837769eb03fdbb231bd/",
];

/// Quick English voices, each a small model of its own.
pub const PIPER: Model = Model {
    id: "piper",
    name: "Piper",
    about: "Quick, clear English voices; each is a small model of its own",
    license: "MIT; voices public domain, CC0 or Apache 2.0",
    hosts: PIPER_HOSTS,
    files: &G2P,
    builds: &[],
    voices: PIPER_VOICES,
    first_voice: "en_US-ljspeech-medium",
    knobs: &[
        Knob {
            id: "expression",
            name: "Expression",
            about: "How much the voice's tone rises and falls",
            range: (0.0, 1.5),
        },
        Knob {
            id: "rhythm",
            name: "Rhythm",
            about: "How freely the timing of words varies",
            range: (0.0, 1.5),
        },
    ],
};

type Listed = (&'static str, u64, &'static str);

/// A Piper voice: its weights, then its settings beside them.
const fn piper(
    (id, name, language): (&'static str, &'static str, &'static str),
    (gender, note): (Gender, &'static str),
    weights: Listed,
    settings: Listed,
) -> ModelVoice {
    ModelVoice {
        id,
        name,
        language,
        gender,
        file: Some(listed(weights)),
        extra: Some(listed(settings)),
        note,
    }
}

const fn listed((path, bytes, sha256): Listed) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256,
        hosts: None,
        from: None,
    }
}

/// Voices whose recordings are public domain, CC0 or Apache 2.0.
const PIPER_VOICES: &[ModelVoice] = &[
    piper(
        ("en_US-ljspeech-medium", "LJ", "en-US"),
        (Gender::Female, ""),
        (
            "en/en_US/ljspeech/medium/en_US-ljspeech-medium.onnx",
            63_531_379,
            "6f52a751e2349abe7a76735eb09dc1875298c77ea2342ffd2fef79ff81b87f22",
        ),
        (
            "en/en_US/ljspeech/medium/en_US-ljspeech-medium.onnx.json",
            4_972,
            "141d612cc0a95ed7efc1ca936b845c2364967f2e9217c5dbfcf69fc4d6c65860",
        ),
    ),
    piper(
        ("en_US-ljspeech-high", "LJ, finer", "en-US"),
        (
            Gender::Female,
            "Finer sound, about six times slower to make",
        ),
        (
            "en/en_US/ljspeech/high/en_US-ljspeech-high.onnx",
            114_199_011,
            "5d4f08ba6a2a48c44592eed3ce56bf85e9de3dd4e20df90541ae68a8310c029a",
        ),
        (
            "en/en_US/ljspeech/high/en_US-ljspeech-high.onnx.json",
            4_970,
            "7e1f4634af596d83cca997fb7a931ba80b70f8a316a2655ee69c55365e0ace14",
        ),
    ),
    piper(
        ("en_US-kristin-medium", "Kristin", "en-US"),
        (Gender::Female, ""),
        (
            "en/en_US/kristin/medium/en_US-kristin-medium.onnx",
            63_531_379,
            "5849957f929cbf720c258f8458692d6103fff2f0e3d3b19c8259474bb06a18d4",
        ),
        (
            "en/en_US/kristin/medium/en_US-kristin-medium.onnx.json",
            4_968,
            "5681426d4aead22195de70531eeeeddb46493cfaffc5764b2ea3db73428b651c",
        ),
    ),
    piper(
        ("en_US-kathleen-low", "Kathleen", "en-US"),
        (Gender::Female, "Quickest to make, a little muffled"),
        (
            "en/en_US/kathleen/low/en_US-kathleen-low.onnx",
            63_104_526,
            "87adf17f5326bc0782282147a8b9788406236245f0f9b0e68dacb651bc1de8b6",
        ),
        (
            "en/en_US/kathleen/low/en_US-kathleen-low.onnx.json",
            4_169,
            "39ebab88416edce0f63781754651788620cb97b9bb73ddf3c01f28e29a648feb",
        ),
    ),
    piper(
        ("en_US-joe-medium", "Joe", "en-US"),
        (Gender::Male, ""),
        (
            "en/en_US/joe/medium/en_US-joe-medium.onnx",
            63_201_294,
            "58afce0321b8d9c46d7cdf9c16500cc55a793b4220212dba6b70fb788b3baf06",
        ),
        (
            "en/en_US/joe/medium/en_US-joe-medium.onnx.json",
            4_794,
            "3d6d5410b3795cb1950595247ef8f06190719e6fdbfa3a2356d8ec368e1aad33",
        ),
    ),
    piper(
        ("en_US-john-medium", "John", "en-US"),
        (Gender::Male, ""),
        (
            "en/en_US/john/medium/en_US-john-medium.onnx",
            63_531_379,
            "789c6c875726e627ddee93d51d8727859abe9c091c3d141591f4b83c2072e988",
        ),
        (
            "en/en_US/john/medium/en_US-john-medium.onnx.json",
            4_965,
            "af60f177b6b550f3d7a302720c0fb89e7f94a82b5dca464775ef63b1c69ba09a",
        ),
    ),
    piper(
        ("en_US-norman-medium", "Norman", "en-US"),
        (Gender::Male, ""),
        (
            "en/en_US/norman/medium/en_US-norman-medium.onnx",
            63_531_379,
            "b9739443232a80a59c7d18810dd856899bf16a7964725f5ab81ea49b1351cb71",
        ),
        (
            "en/en_US/norman/medium/en_US-norman-medium.onnx.json",
            4_968,
            "6c2db7f558a4a8deb9fe822583c1c5105f6c4e834dd0f9de8ad17a888ee9fe1d",
        ),
    ),
    piper(
        ("en_US-bryce-medium", "Bryce", "en-US"),
        (Gender::Male, ""),
        (
            "en/en_US/bryce/medium/en_US-bryce-medium.onnx",
            63_531_379,
            "dc9caa6c313199ffb5ac698b6e542fa6cba388aeaf2731e25262e33b9810aef1",
        ),
        (
            "en/en_US/bryce/medium/en_US-bryce-medium.onnx.json",
            4_966,
            "7ceb1bc4af6d4e41b6d1edbb86c67e91e01eaa71f66db4cd0ae92ac704d415be",
        ),
    ),
    piper(
        ("en_US-mike-medium", "Mike", "en-US"),
        (Gender::Male, ""),
        (
            "en/en_US/mike/medium/en_US-mike-medium.onnx",
            63_221_984,
            "3157e69e2c580cab297483f4b301ee975a4590c17d35a9757e33347d0e90fae2",
        ),
        (
            "en/en_US/mike/medium/en_US-mike-medium.onnx.json",
            5_439,
            "0a2538c0b1e06a6cab7c46b97c79abd5e6a34a6c05b74b9376874c7099d81dad",
        ),
    ),
    piper(
        ("en_US-reza_ibrahim-medium", "Reza", "en-US"),
        (Gender::Male, ""),
        (
            "en/en_US/reza_ibrahim/medium/en_US-reza_ibrahim-medium.onnx",
            63_511_038,
            "99f0c31464a2120831ca87d079e10a9a2b3e426cc1ee662d80ff9042df15cd3c",
        ),
        (
            "en/en_US/reza_ibrahim/medium/en_US-reza_ibrahim-medium.onnx.json",
            4_976,
            "465ddf1702917fe617b7d69ed81301d6a2f39f083a754bd1cf6db8955d09a381",
        ),
    ),
    piper(
        ("en_US-sam-medium", "Sam", "en-US"),
        (Gender::Neutral, ""),
        (
            "en/en_US/sam/medium/en_US-sam-medium.onnx",
            62_950_044,
            "56417b3b4afe8ec6bb4cabf06e17d67261fdd5bf334592abcfc80052fba11163",
        ),
        (
            "en/en_US/sam/medium/en_US-sam-medium.onnx.json",
            5_040,
            "8c7fb47f19683b0b81037c5564f9a5ad4699a9da685e0e5da0a72fd3c3f5c1c4",
        ),
    ),
    piper(
        ("en_GB-cori-medium", "Cori", "en-GB"),
        (Gender::Female, ""),
        (
            "en/en_GB/cori/medium/en_GB-cori-medium.onnx",
            63_531_379,
            "1899f98e5fb8310154f3c2973f4b8a929ba7245e722b3d3a85680b833d95f10d",
        ),
        (
            "en/en_GB/cori/medium/en_GB-cori-medium.onnx.json",
            4_966,
            "e262c16d7f192f69d4edd6b4ef8a5915379e67495fcc402f1ab15eeb33da3d36",
        ),
    ),
    piper(
        ("en_GB-cori-high", "Cori, finer", "en-GB"),
        (
            Gender::Female,
            "Finer sound, about six times slower to make",
        ),
        (
            "en/en_GB/cori/high/en_GB-cori-high.onnx",
            114_219_352,
            "470b4dd634c98f8a4850d7626ffc3dfc90774628eeef6605a6dd8f88f30a5903",
        ),
        (
            "en/en_GB/cori/high/en_GB-cori-high.onnx.json",
            4_963,
            "9e7fb5b5671612c22f3c81cbe46c1ae87b031a4632bcb509e499dad6f1e2adec",
        ),
    ),
];

/// Each Kitten size is its own repository, every one pinned.
macro_rules! kitten_hosts {
    ($repo:literal, $rev:literal) => {
        &[
            concat!(
                "https://huggingface.co/KittenML/",
                $repo,
                "/resolve/",
                $rev,
                "/"
            ),
            concat!(
                "https://hf-mirror.com/KittenML/",
                $repo,
                "/resolve/",
                $rev,
                "/"
            ),
        ]
    };
}

const KITTEN_NANO: &[&str] = kitten_hosts!(
    "kitten-tts-nano-0.8-fp32",
    "7a1db645b1f3ab9420761d87428e042b9cec3f26"
);
const KITTEN_NANO8: &[&str] = kitten_hosts!(
    "kitten-tts-nano-0.8-int8",
    "84781d74e29ee25217551556398b42f80593a813"
);
const KITTEN_MICRO: &[&str] = kitten_hosts!(
    "kitten-tts-micro-0.8",
    "1ccf72b2c2048fd17efac7de2fab32d10e225084"
);
const KITTEN_MINI: &[&str] = kitten_hosts!(
    "kitten-tts-mini-0.8",
    "c02725660cea441db4c383af69f1f26f5cd00947"
);

/// A Kitten file kept as `path`, fetched as `from` off `hosts`.
const fn kitten_file(
    (path, from): (&'static str, &'static str),
    hosts: &'static [&'static str],
    (bytes, sha256): (u64, &'static str),
) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256,
        hosts: Some(hosts),
        from: Some(from),
    }
}

/// The nano builds' voices: one file, so both share it.
const NANO_VOICES: ModelFile = kitten_file(
    ("nano-voices.npz", "voices.npz"),
    KITTEN_NANO,
    (
        3_278_902,
        "8aa7cee235abb0739cb51e6559685f65a4dacd95568833d05699b1633f519b3f",
    ),
);

/// Small English models that run on the computer; every build has every voice.
pub const KITTEN: Model = Model {
    id: "kitten",
    name: "Kitten",
    about: "Light English voices from tiny models, 15 to 80 million parameters",
    license: "Apache 2.0",
    hosts: KITTEN_NANO,
    // American English is all Kitten speaks.
    files: &[G2P[0], G2P[1], G2P[2], G2P[3], G2P[4]],
    builds: &[
        Build {
            id: "mini",
            name: "Mini",
            about: "80 million parameters: the best sound",
            file: kitten_file(
                ("mini.onnx", "kitten_tts_mini_v0_8.onnx"),
                KITTEN_MINI,
                (
                    78_268_016,
                    "0f5bbae4fc4800c98dbc544a87ecfa79510de2fb8222db30d12e5bfe9177df91",
                ),
            ),
            extra: Some(kitten_file(
                ("mini-voices.npz", "voices.npz"),
                KITTEN_MINI,
                (
                    3_278_902,
                    "40ad2638952b77b7b2f30127e2608e169fc69dd256b53bd8aaa3409a33193c42",
                ),
            )),
        },
        Build {
            id: "micro",
            name: "Micro",
            about: "40 million parameters: near the best, sooner heard",
            file: kitten_file(
                ("micro.onnx", "kitten_tts_micro_v0_8.onnx"),
                KITTEN_MICRO,
                (
                    41_384_970,
                    "95481626fee1ba70ce683e69c534fc7cb38433c46ce42d3abbeafb4b9f1a4123",
                ),
            ),
            extra: Some(kitten_file(
                ("micro-voices.npz", "voices.npz"),
                KITTEN_MICRO,
                (
                    3_278_902,
                    "112710c1be8ad0e967c190fb0fd95cbe5848ec4791b93209f20b28b7da20dac1",
                ),
            )),
        },
        Build {
            id: "nano",
            name: "Nano",
            about: "15 million parameters: the quickest",
            file: kitten_file(
                ("nano.onnx", "kitten_tts_nano_v0_8.onnx"),
                KITTEN_NANO,
                (
                    56_767_095,
                    "320564d2615f235de972ca27a7f39551c94185cfa24ca85b07a29084135f1e5e",
                ),
            ),
            extra: Some(NANO_VOICES),
        },
        Build {
            id: "nano8",
            name: "Nano 8-bit",
            about: "Nano in 8-bit weights: the smallest download, a little rougher",
            file: kitten_file(
                ("nano-8bit.onnx", "kitten_tts_nano_v0_8.onnx"),
                KITTEN_NANO8,
                (
                    24_369_971,
                    "f7b0afcbee92870b32b8e0276d855b954dc25470c9f051b376ac7eee537c76fc",
                ),
            ),
            extra: Some(NANO_VOICES),
        },
    ],
    voices: KITTEN_VOICES,
    first_voice: "expr-voice-2-f",
    knobs: &[],
};

/// A voice inside every Kitten build.
const fn kitten_voice(id: &'static str, name: &'static str, gender: Gender) -> ModelVoice {
    ModelVoice {
        id,
        name,
        language: "en-US",
        gender,
        file: None,
        extra: None,
        note: "",
    }
}

const KITTEN_VOICES: &[ModelVoice] = &[
    kitten_voice("expr-voice-2-f", "Bella", Gender::Female),
    kitten_voice("expr-voice-2-m", "Jasper", Gender::Male),
    kitten_voice("expr-voice-3-f", "Luna", Gender::Female),
    kitten_voice("expr-voice-3-m", "Bruno", Gender::Male),
    kitten_voice("expr-voice-4-f", "Rosie", Gender::Female),
    kitten_voice("expr-voice-4-m", "Hugo", Gender::Male),
    kitten_voice("expr-voice-5-f", "Kiki", Gender::Female),
    kitten_voice("expr-voice-5-m", "Leo", Gender::Male),
];

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn every_file_is_listed_with_a_size_and_a_checksum() {
        for model in MODELS {
            assert!(
                model.first().is_some(),
                "{} lacks its first voice",
                model.id
            );
            let all = (model.files.iter().copied())
                .chain(model.builds.iter().flat_map(|b| b.files()))
                .chain(model.voices.iter().flat_map(|v| v.files()));
            for f in all {
                assert!(f.bytes > 0, "{}", f.path);
                assert_eq!(f.sha256.len(), 64, "{}", f.path);
                assert!(f.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            }
            for v in model.voices {
                assert_eq!(model.voices.iter().filter(|w| w.id == v.id).count(), 1);
            }
            for b in model.builds {
                assert_eq!(model.builds.iter().filter(|c| c.id == b.id).count(), 1);
            }
            for f in (model.core().into_iter())
                .chain(model.builds.iter().flat_map(|b| b.files()))
                .chain(model.voices.iter().flat_map(|v| v.files()))
            {
                let urls = model.urls(&f);
                assert!(urls.len() > 1, "{} has no mirror", f.path);
                assert!(urls.iter().all(|u| u.starts_with("https://")));
                let name = f.from.unwrap_or(f.path);
                assert!(urls.iter().all(|u| u.ends_with(name)));
            }
        }
    }

    #[test]
    fn the_model_downloads_with_one_voice_and_a_build_of_its_choosing() {
        let core = KOKORO.core();
        assert_eq!(core.len(), 10);
        assert!(core.iter().any(|f| f.path == "voices/af_heart.pt"));
        assert!(core.iter().all(|f| !f.path.contains("kokoro-")));
        let g2p = 20_120_816;
        assert_eq!(KOKORO.bytes(), 2_351 + 523_425 + g2p);
        let ids: Vec<&str> = KOKORO.builds.iter().map(|b| b.id).collect();
        assert_eq!(ids, ["full", "half", "small"]);
        let small = KOKORO.build("small").unwrap();
        assert_eq!(KOKORO.bytes_with(small), KOKORO.bytes() + 141_322_336);
        // Each smaller build is a smaller download.
        let sizes: Vec<u64> = KOKORO.builds.iter().map(|b| b.file.bytes).collect();
        assert!(sizes.windows(2).all(|w| w[0] > w[1]));
        assert_eq!(KOKORO.voices.len(), 41);
        for v in KOKORO.voices {
            let file = v.file.unwrap();
            assert_eq!(v.files(), [file]);
            assert_eq!(file.path, format!("voices/{}.pt", v.id));
        }
    }

    #[test]
    fn each_piper_voice_brings_its_weights_and_settings() {
        assert!(PIPER.builds.is_empty());
        assert_eq!(PIPER.files, &G2P[..]);
        let first = PIPER.first().unwrap();
        assert_eq!(PIPER.core().len(), G2P.len() + 2);
        assert_eq!(PIPER.bytes(), 20_120_816 + first.bytes());
        for v in PIPER.voices {
            let [weights, settings] = v.files()[..] else {
                panic!("{} has two files", v.id);
            };
            // As `en/en_US/joe/medium/en_US-joe-medium.onnx`.
            let [locale, speaker, quality] = v.id.splitn(3, '-').collect::<Vec<_>>()[..] else {
                panic!("{} is locale-speaker-quality", v.id);
            };
            let stem = format!("en/{locale}/{speaker}/{quality}/{}", v.id);
            assert_eq!(weights.path, format!("{stem}.onnx"));
            assert_eq!(settings.path, format!("{stem}.onnx.json"));
            assert_eq!(v.language, locale.replace('_', "-"));
            // High voices are slow to make, and say so.
            assert_eq!(
                quality == "high",
                !v.note.is_empty() && v.note.contains("slower")
            );
        }
        let knob = PIPER.knobs[0];
        assert_eq!(knob.clamp(None), 1.0);
        assert_eq!(knob.clamp(Some(f32::NAN)), 1.0);
        assert_eq!(knob.clamp(Some(9.0)), knob.range.1);
    }

    #[test]
    fn each_kitten_build_brings_its_voices_and_every_voice_comes_with_it() {
        assert_eq!(KITTEN.files, &G2P[..5]);
        assert_eq!(KITTEN.bytes(), 10_606_674);
        let ids: Vec<&str> = KITTEN.builds.iter().map(|b| b.id).collect();
        assert_eq!(ids, ["mini", "micro", "nano", "nano8"]);
        let nano = |id| KITTEN.build(id).unwrap().extra;
        assert_eq!(nano("nano"), nano("nano8"), "one voices file for both");
        assert_eq!(nano("nano").unwrap().path, "nano-voices.npz");
        for b in KITTEN.builds {
            let [onnx, voices] = b.files()[..] else {
                panic!("{} brings weights and voices", b.id);
            };
            assert!(onnx.path.ends_with(".onnx") && voices.path.ends_with(".npz"));
            assert_eq!(voices.from, Some("voices.npz"));
            assert_eq!(b.bytes(), onnx.bytes + 3_278_902);
        }
        let url = KITTEN.urls(&KITTEN.build("micro").unwrap().file);
        assert!(url[0].ends_with("kitten-tts-micro-0.8/resolve/1ccf72b2c2048fd17efac7de2fab32d10e225084/kitten_tts_micro_v0_8.onnx"));
        assert_eq!(KITTEN.voices.len(), 8);
        assert!(KITTEN.voices.iter().all(|v| v.files().is_empty()));
        assert!(KITTEN.core().iter().all(|f| KITTEN.files.contains(f)));
    }

    #[test]
    fn the_models_bring_every_file_their_pronunciation_reads() {
        use mareader_g2p::{Dialect, TAGGER, TOKENIZER};
        let dialects = [Dialect::American, Dialect::British];
        let needed = dialects.iter().flat_map(|d| d.files());
        for name in needed.chain([TOKENIZER, TAGGER]) {
            for model in [KOKORO, PIPER] {
                assert!(model.files.iter().any(|f| f.path == name), "{name}");
            }
        }
    }
}
