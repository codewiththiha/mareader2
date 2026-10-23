"""Piper's own models run by onnxruntime, and espeak's phonemes, for piper-parity.

Usage: reference.py <voices dir> <out dir>   (after `piper-parity prepare`)
"""
import json
import os
import sys
import time

import numpy as np
import onnxruntime as ort
from piper.phonemize_espeak import EspeakPhonemizer

voices, out = sys.argv[1], sys.argv[2]
cases = json.load(open(os.path.join(out, "cases.json"), encoding="utf-8"))
os.makedirs(os.path.join(out, "ref"), exist_ok=True)
espeak = EspeakPhonemizer()
options = ort.SessionOptions()
options.intra_op_num_threads = os.cpu_count() or 1
sessions, configs = {}, {}
timing, phonemes = {}, {}
for case in cases:
    voice = case["voice"]
    if voice not in sessions:
        path = os.path.join(voices, voice + ".onnx")
        sessions[voice] = ort.InferenceSession(path, options, providers=["CPUExecutionProvider"])
        configs[voice] = json.load(open(path + ".json", encoding="utf-8"))
        timing[voice] = {"seconds": 0.0, "samples": 0}
        phonemes[voice] = {}
    cfg = configs[voice]
    inf = cfg["inference"]
    ids = np.array([case["ids"]], dtype=np.int64)
    for kind, scales in (("det", [0.0, inf["length_scale"], 0.0]),
                         ("noisy", [inf["noise_scale"], inf["length_scale"], inf["noise_w"]])):
        feed = {"input": ids, "input_lengths": np.array([ids.shape[1]], dtype=np.int64),
                "scales": np.array(scales, dtype=np.float32)}
        if cfg.get("num_speakers", 1) > 1:
            feed["sid"] = np.array([case["speaker"]], dtype=np.int64)
        start = time.perf_counter()
        audio = sessions[voice].run(None, feed)[0].reshape(-1).astype("<f4")
        timing[voice]["seconds"] += time.perf_counter() - start
        timing[voice]["samples"] += audio.size
        audio.tofile(os.path.join(out, "ref", f"{voice}.{case['index']}.{kind}.f32"))
    said = espeak.phonemize(cfg["espeak"]["voice"], case["text"])
    phonemes[voice][str(case["index"])] = "".join("".join(s) for s in said)
json.dump({"timing": timing, "espeak": phonemes},
          open(os.path.join(out, "ref", "reference.json"), "w", encoding="utf-8"),
          ensure_ascii=False, indent=1)
print(f"reference: {len(cases)} sentences")
