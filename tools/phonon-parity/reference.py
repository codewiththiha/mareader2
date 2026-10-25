"""Reference side of phonon-parity: ptts audio and text, numpy latents, HF tokens.

Usage: reference.py <model> <voices> <out>. Reads <out>/cases.json and
writes <out>/ref/.
"""

import json
import sys
import time
from pathlib import Path

import numpy as np
import ptts
from tokenizers import Tokenizer

sys.path.insert(0, str(Path(__file__).parent))
from numpy_ref import Model, read_safetensors  # noqa: E402

STAGE_CASES = (1, 3)
QUANTS = ("f32", "q8_0")


def voice_file(model, voices, name):
    if name == "default":
        return model / "default-voice.safetensors"
    return voices / f"{name}.safetensors"


def main():
    model, voices, out = (Path(a) for a in sys.argv[1:4])
    ref = out / "ref"
    ref.mkdir(parents=True, exist_ok=True)
    cases = json.loads((out / "cases.json").read_text())

    tok = Tokenizer.from_file(str(model / "tokenizer.json"))
    ids = [tok.encode(text, add_special_tokens=False).ids for text, _ in cases["tokens"]]
    decoded = [tok.decode(i, skip_special_tokens=True) for i in ids]

    timing, load = {}, {}
    names = sorted({c["voice"] for c in cases["cases"]})
    for quant in QUANTS:
        start = time.perf_counter()
        tts = ptts.TTS(model=str(model), quant=quant, temperature=0.0, lang="en")
        for name in names:
            if name != "default":
                tts.add_voice(name, str(voice_file(model, voices, name)))
        load[quant] = time.perf_counter() - start
        seconds, samples = 0.0, 0
        build = "f32" if quant == "f32" else "q8"
        for case in cases["cases"]:
            start = time.perf_counter()
            audio = np.asarray(tts.synth(case["text"], voice=case["voice"]), dtype=np.float32)
            seconds += time.perf_counter() - start
            samples += len(audio)
            audio.astype("<f4").tofile(ref / f"{build}.{case['index']}.f32")
        timing[quant] = {"seconds": seconds, "samples": samples}
        del tts

    numpy_model = Model(model / "model.safetensors", model / "config.json")
    for case in cases["cases"]:
        if case["index"] not in STAGE_CASES:
            continue
        chunk = case["chunks"][0]
        wavs = read_safetensors(voice_file(model, voices, case["voice"]))["speaker_wavs"]
        latents, _ = numpy_model.run(
            wavs[0].T, chunk["tokens"], chunk["frame_budget"], chunk["frames_after_eos"]
        )
        latents.astype("<f4").tofile(ref / f"latents.{case['index']}.f32")

    normalized = [ptts.normalize(text, lang="en") for text, _ in cases["normalized"]]
    reference = {
        "timing": timing,
        "tokens": ids,
        "decoded": decoded,
        "load": load,
        "normalized": normalized,
    }
    (ref / "reference.json").write_text(json.dumps(reference))
    print("reference written:", {k: round(v["seconds"], 2) for k, v in timing.items()})


if __name__ == "__main__":
    main()
