"""Kitten's own builds run by onnxruntime, and espeak's phonemes, for kitten-parity.

Usage: reference.py <builds dir> <out dir>   (after `kitten-parity prepare`)
Each build is a folder holding model.onnx and voices.npz.
"""
import json
import os
import sys
import time

import numpy as np
import onnx
import onnxruntime as ort
from onnx import helper, numpy_helper
from phonemizer.backend import EspeakBackend

MASK = (1 << 64) - 1


def offset(h):
    """The port's fixed phase offset for harmonic h, in place of a random one."""
    v = (h + 0x9E3779B97F4A7C15) & MASK
    v = ((v ^ (v >> 30)) * 0xBF58476D1CE4E5B9) & MASK
    v = ((v ^ (v >> 27)) * 0x94D049BB133111EB) & MASK
    v ^= v >> 31
    return ((v >> 40) & 0xFFFFFFFF) / float(1 << 24)


GOLDEN = np.uint64(0x9E3779B97F4A7C15)
NOISE_SECONDS = 40


def noise(count):
    """The port's seeded normal draws, seed 0: SplitMix64 paired by Box-Muller."""
    with np.errstate(over="ignore"):
        z = np.arange(1, 2 * count + 1, dtype=np.uint64) * GOLDEN
        z = (z ^ (z >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
        z = (z ^ (z >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
        z ^= z >> np.uint64(31)
    unit = ((z >> np.uint64(11)).astype(np.float64) + 1.0) / float(1 << 53)
    radius = np.sqrt(-2.0 * np.log(unit[0::2]))
    angle = 2.0 * np.pi * unit[1::2]
    draws = np.empty(2 * count, dtype=np.float32)
    draws[0::2] = (radius * np.cos(angle)).astype(np.float32)
    draws[1::2] = (radius * np.sin(angle)).astype(np.float32)
    return draws


GEN = "/decoder/generator/"
# Our probe names against the reference graph's tensors, for one sentence.
STAGES = {
    "x0": "/decoder/decode.3/Mul_output_0",
    "source": GEN + "m_source/l_tanh/Tanh_output_0",
    "har": GEN + "Concat_1_output_0",
    "stage0.src": GEN + "noise_res.0/Add_8_output_0",
    "stage0.sum": GEN + "Add_3_output_0",
    "stage0.out": GEN + "Div_1_output_0",
    "stage1.src": GEN + "noise_res.1/Add_8_output_0",
    "stage1.sum": GEN + "Add_5_output_0",
    "stage1.out": GEN + "Div_2_output_0",
    "post": GEN + "conv_post/Conv_output_0",
}
STAGE_BUILD, STAGE_CASE = "nano", 1


def ints(name, value):
    """A one-number int64 tensor."""
    return helper.make_tensor(name, onnx.TensorProto.INT64, [1], [value])


def quiet(path, stages=False):
    """The graph with the port's noise and random phases fed in."""
    model = onnx.load(path)
    nodes = []
    for n in model.graph.node:
        out = n.output[0]
        if n.op_type == "RandomNormalLike":
            # Sines are [1, samples, 9]; the draws are cut to that many samples.
            nodes += [
                helper.make_node("Shape", [n.input[0]], [out + "_shape"]),
                helper.make_node("Constant", [], [out + "_one"], value=ints(out + "_1", 1)),
                helper.make_node("Constant", [], [out + "_two"], value=ints(out + "_2", 2)),
                helper.make_node("Constant", [], [out + "_zero"], value=ints(out + "_0", 0)),
                helper.make_node("Slice", [out + "_shape", out + "_one", out + "_two"],
                                 [out + "_len"]),
                helper.make_node("Slice", ["parity_noise", out + "_zero", out + "_len",
                                           out + "_one"], [out]),
            ]
        elif n.op_type == "RandomUniformLike":
            phases = np.array([[offset(h) for h in range(9)]], dtype=np.float32)
            nodes.append(helper.make_node("Constant", [], [out],
                                          value=numpy_helper.from_array(phases, out + "_phases")))
        else:
            nodes.append(n)
    del model.graph.node[:]
    model.graph.node.extend(nodes)
    model.graph.input.append(helper.make_tensor_value_info(
        "parity_noise", onnx.TensorProto.FLOAT, [1, "samples", 9]))
    if stages:
        made = {o for n in model.graph.node for o in n.output}
        for probe, name in STAGES.items():
            if name in made:
                model.graph.output.append(onnx.ValueInfoProto(name=name))
            else:
                print(f"stages: no {name} for {probe}")
    return model.SerializeToString()


def dump_stages(path, feed, out):
    """The reference's tensors at our probes, as raw f32 with their shapes."""
    session = ort.InferenceSession(quiet(path, stages=True), options,
                                   providers=["CPUExecutionProvider"])
    names = [o.name for o in session.get_outputs()]
    folder = os.path.join(out, "ref", "stages")
    os.makedirs(folder, exist_ok=True)
    shapes = {}
    for name, value in zip(names, session.run(None, feed)):
        probe = next((p for p, n in STAGES.items() if n == name), None)
        if probe is not None:
            value = np.asarray(value, dtype="<f4")
            shapes[probe] = list(value.shape)
            value.reshape(-1).tofile(os.path.join(folder, probe + ".f32"))
    json.dump(shapes, open(os.path.join(folder, "shapes.json"), "w"))


builds, out = sys.argv[1], sys.argv[2]
cases = json.load(open(os.path.join(out, "cases.json"), encoding="utf-8"))
os.makedirs(os.path.join(out, "ref"), exist_ok=True)
options = ort.SessionOptions()
options.intra_op_num_threads = os.cpu_count() or 1
timing = {}
draws = noise(NOISE_SECONDS * 24000 * 9 // 2).reshape(1, -1, 9)
for build in sorted(os.listdir(builds)):
    folder = os.path.join(builds, build)
    if not os.path.exists(os.path.join(folder, "model.onnx")):
        continue
    session = ort.InferenceSession(quiet(os.path.join(folder, "model.onnx")), options,
                                   providers=["CPUExecutionProvider"])
    voices = np.load(os.path.join(folder, "voices.npz"))
    timing[build] = {"seconds": 0.0, "samples": 0}
    for case in cases:
        rows = voices[case["voice"]]
        style = rows[min(len(case["text"]), rows.shape[0] - 1)][None, :].astype(np.float32)
        feed = {"input_ids": np.array([case["ids"]], dtype=np.int64), "style": style,
                "speed": np.array([1.0], dtype=np.float32), "parity_noise": draws}
        start = time.perf_counter()
        audio, duration = session.run(None, feed)
        timing[build]["seconds"] += time.perf_counter() - start
        audio = audio.reshape(-1).astype("<f4")
        assert audio.size < draws.shape[1], f"{build} case {case['index']} outran the noise"
        timing[build]["samples"] += audio.size
        stem = os.path.join(out, "ref", f"{build}.{case['index']}")
        audio.tofile(stem + ".f32")
        json.dump([int(d) for d in np.asarray(duration).reshape(-1)], open(stem + ".dur.json", "w"))
        if (build, case["index"]) == (STAGE_BUILD, STAGE_CASE):
            dump_stages(os.path.join(folder, "model.onnx"), feed, out)
espeak = EspeakBackend("en-us", preserve_punctuation=True, with_stress=True)
said = espeak.phonemize([c["text"] for c in cases])
phonemes = {str(c["index"]): p for c, p in zip(cases, said)}
json.dump({"timing": timing, "espeak": phonemes},
          open(os.path.join(out, "ref", "reference.json"), "w", encoding="utf-8"),
          ensure_ascii=False, indent=1)
print(f"reference: {len(cases)} sentences on {len(timing)} builds")
