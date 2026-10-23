#!/usr/bin/env python3
"""mareader-hanzi against misaki's Chinese reader, as Kokoro-82M runs it.

  python3 tools/hanzi-parity/check.py BIN OUT

BIN is the crate's `hanzi` example: a data folder, lines on stdin, Kokoro's
phonemes out. The data is fetched here from the commits the app pins, sizes
and checksums checked. misaki's side is its released ZHG2P() with jieba,
pypinyin and cn2an at those same versions; both sides keep only Kokoro's
vocabulary. Python and misaki run in CI only, never in the app.
"""

import collections
import hashlib
import os
import subprocess
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
MOST = 0.1  # phoneme error allowed, in percent

PYPINYIN = "https://raw.githubusercontent.com/mozillazg/python-pinyin/df101577145af2eb1abe5656e592e34e3bb56d23/pypinyin/"
JIEBA = "https://raw.githubusercontent.com/fxsjy/jieba/1e20c89b66f56c9301b0feed211733ffaa1bd72a/jieba/"

# pypinyin 0.55.0 and jieba 0.42.1, as the app downloads them.
DATA = [
    ("pinyin_dict.json", PYPINYIN + "pinyin_dict.json", 788_780,
     "5f294c01e6c6c0a1c8e329c79335a3f8e0b27d06bf1de7a99244b765892d1e5b"),
    ("phrases_dict.json", PYPINYIN + "phrases_dict.json", 2_545_585,
     "a45ff140a6b631ca9c82127b280a2f414e0aba6bb2824a0e9d1e77fff359c665"),
    ("dict.txt", JIEBA + "dict.txt", 5_071_852,
     "7197c3211ddd98962b036cdf40324d1ea2bfaa12bd028e68faa70111a88e12a8"),
    ("prob_emit.py", JIEBA + "finalseg/prob_emit.py", 1_321_732,
     "27d46b1c9efe4dd148fde8be042a21be40e3562d0c7f1273f9de7abae12ebb8d"),
]

# Kokoro-82M's vocabulary, from its config.json.
VOCAB = set(
    " !\"(),.:;?AIOQSTWYabcdefhijklmnopqrstuvwxyzæçðøŋœɐɑɒɔɕɖəɚɛɜɟɡɣɤɥɨɪɯɰɲ"
    "ɳɴɸɹɻɽɾʁʂʃʈʊʋʌʎʒʔʝʣʤʥʦʧʨʰʲˈˌː̃βθχᵊᵝᵻ—“”…→↓↗↘ꭧ"
)


def normalise(phonemes: str) -> str:
    """As Kokoro's normaliser: known symbols only, single spaces."""
    out = []
    for ch in phonemes.replace("g", "ɡ"):
        ch = " " if ch.isspace() else ch
        if ch in VOCAB and not (ch == " " and out and out[-1] == " "):
            out.append(ch)
    return "".join(out).strip()


def fetch(folder: str) -> None:
    os.makedirs(folder, exist_ok=True)
    for name, url, size, sha in DATA:
        with urllib.request.urlopen(url, timeout=60) as response:
            body = response.read()
        if len(body) != size or hashlib.sha256(body).hexdigest() != sha:
            sys.exit(f"{name}: not the pinned file")
        with open(os.path.join(folder, name), "wb") as f:
            f.write(body)


def misaki(lines: list[str]) -> list[str]:
    from misaki.zh import ZHG2P

    g2p = ZHG2P()
    return [normalise(g2p(line)[0]) for line in lines]


def ours(binary: str, folder: str, lines: list[str]) -> list[str]:
    text = "\n".join(lines) + "\n"
    run = subprocess.run([binary, folder], input=text, capture_output=True,
                         text=True, check=True)
    return [normalise(line) for line in run.stdout.split("\n")[: len(lines)]]


def distance(a: str, b: str) -> int:
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def main() -> int:
    binary, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    folder = os.path.join(out, "data")
    fetch(folder)
    with open(os.path.join(HERE, "zh.txt"), encoding="utf-8") as f:
        lines = [line.strip() for line in f if line.strip() and not line.startswith("#")]
    want, got = misaki(lines), ours(binary, folder, lines)
    length = errors = same = 0
    words = collections.Counter()
    for w, g in zip(want, got):
        length += len(w)
        errors += distance(w, g)
        same += w == g
        ww, gw = w.split(" "), g.split(" ")
        if len(ww) == len(gw):
            words.update((a, b) for a, b in zip(ww, gw) if a != b)
    rate = 100 * errors / max(length, 1)
    held = rate <= MOST
    report = [
        "## mareader-hanzi against misaki\n",
        f"- {len(lines)} sentences",
        f"- Phoneme error: **{rate:.3f}%** (at most {MOST}%): "
        + ("held" if held else "**no**"),
        f"- Sentences exactly as misaki: {same} of {len(lines)}\n",
    ]
    if words:
        report += ["| Times | misaki | ours |", "|---|---|---|"]
        report += [f"| {n} | {a} | {b} |" for (a, b), n in words.most_common(15)]
        report.append("")
    report.append("**All checks pass.**" if held else "**Some checks fail.**")
    with open(os.path.join(out, "summary.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(report) + "\n")
    print("\n".join(report))
    return 0 if held else 1


if __name__ == "__main__":
    sys.exit(main())
