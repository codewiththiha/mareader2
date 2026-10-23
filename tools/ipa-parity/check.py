#!/usr/bin/env python3
"""mareader-ipa against espeak-ng, as Kokoro's voices read both.

  python3 tools/ipa-parity/check.py BIN OUT

BIN is the crate's `ipa` example: lines on stdin, Kokoro's phonemes out.
espeak-ng speaks the same lines through phonemizer, with the steps misaki
takes for Kokoro; both sides then keep only what Kokoro's vocabulary holds.
The phoneme error rate is the edit distance over espeak's length. espeak-ng
and phonemizer run in CI only, never in the app.
"""

import collections
import os
import subprocess
import sys

from phonemizer.backend import EspeakBackend

HERE = os.path.dirname(os.path.abspath(__file__))

# Language tag, espeak's voice, and the most phoneme error allowed.
LANGUAGES = [("es", "es", 0.5), ("it", "it", 5.0)]

# Kokoro-82M's vocabulary, from its config.json.
VOCAB = set(
    " !\"(),.:;?AIOQSTWYabcdefhijklmnopqrstuvwxyzæçðøŋœɐɑɒɔɕɖəɚɛɜɟɡɣɤɥɨɪɯɰɲ"
    "ɳɴɸɹɻɽɾʁʂʃʈʊʋʌʎʒʔʝʣʤʥʦʧʨʰʲˈˌː̃βθχᵊᵝᵻ—“”…→↓↗↘ꭧ"
)

# misaki's espeak ties, each written as one letter for Kokoro.
TIES = sorted({
    "a^ɪ": "I", "a^ʊ": "W", "d^z": "ʣ", "d^ʒ": "ʤ", "e^ɪ": "A", "o^ʊ": "O",
    "ə^ʊ": "Q", "s^s": "S", "t^s": "ʦ", "t^ʃ": "ʧ", "ɔ^ɪ": "Y",
}.items())


def normalise(phonemes: str) -> str:
    """As Kokoro's normaliser: known symbols only, single spaces."""
    out = []
    for ch in phonemes.replace("g", "ɡ"):
        ch = " " if ch.isspace() else ch
        if ch in VOCAB and not (ch == " " and out and out[-1] == " "):
            out.append(ch)
    return "".join(out).strip()


def espeak(voice: str, lines: list[str]) -> list[str]:
    backend = EspeakBackend(voice, preserve_punctuation=True, with_stress=True,
                            tie="^", language_switch="remove-flags")
    table = str.maketrans({"«": "“", "»": "”", "(": "«", ")": "»"})
    said = backend.phonemize([line.translate(table) for line in lines])
    out = []
    for phonemes in said:
        for tie, letter in TIES:
            phonemes = phonemes.replace(tie, letter)
        phonemes = phonemes.replace("^", "").replace("-", "")
        out.append(normalise(phonemes.replace("«", "(").replace("»", ")")))
    return out


def ours(binary: str, tag: str, lines: list[str]) -> list[str]:
    text = "\n".join(lines) + "\n"
    run = subprocess.run([binary, tag], input=text, capture_output=True,
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


def check(binary: str, tag: str, voice: str, most: float, report: list[str]) -> bool:
    path = os.path.join(HERE, f"{tag}.txt")
    with open(path, encoding="utf-8") as f:
        lines = [line.strip() for line in f if line.strip() and not line.startswith("#")]
    want, got = espeak(voice, lines), ours(binary, tag, lines)
    length = errors = unstressed = same = 0
    words = collections.Counter()
    plain = str.maketrans("", "", "ˈˌ")
    for w, g in zip(want, got):
        length += len(w)
        errors += distance(w, g)
        unstressed += distance(w.translate(plain), g.translate(plain))
        same += w == g
        ww, gw = w.split(" "), g.split(" ")
        if len(ww) == len(gw):
            words.update((a, b) for a, b in zip(ww, gw) if a != b)
    rate = 100 * errors / max(length, 1)
    held = rate <= most
    report += [
        f"## {tag}: {len(lines)} sentences\n",
        f"- Phoneme error: **{rate:.2f}%** (at most {most}%): "
        + ("held" if held else "**no**"),
        f"- Without stress marks: {100 * unstressed / max(length, 1):.2f}%",
        f"- Sentences exactly as espeak: {same} of {len(lines)}\n",
        "| Times | espeak | ours |",
        "|---|---|---|",
    ]
    report += [f"| {n} | {a} | {b} |" for (a, b), n in words.most_common(15)]
    report.append("")
    return held


def main() -> int:
    binary, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    report = ["## mareader-ipa against espeak-ng\n"]
    held = [check(binary, tag, voice, most, report) for tag, voice, most in LANGUAGES]
    version = subprocess.run(["espeak-ng", "--version"], capture_output=True, text=True)
    report.append(f"Reference: {version.stdout.strip()}\n")
    report.append("**All checks pass.**" if all(held) else "**Some checks fail.**")
    with open(os.path.join(out, "summary.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(report) + "\n")
    print("\n".join(report))
    return 0 if all(held) else 1


if __name__ == "__main__":
    sys.exit(main())
