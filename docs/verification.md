# Verification: local mode and CI mode

Every change is proven by the same gates (AGENTS.md, "Gates"): format,
clippy, tests, the xtask checks, and a look at the result. What differs
between machines is **where** they run. Neither mode is fixed: an agent
chooses per session, and the user can override the choice at any time.

| | Local mode | CI mode |
|---|---|---|
| Builds and tests | on this machine | on GitHub Actions |
| Verdict | the local gates | `python3 tools/ci.py watch` |
| Screenshots | `cargo run -p mareader-ui --example shot` | `python3 tools/ci.py shots` |
| Browser preview | `tools/web.sh --serve` | `python3 tools/ci.py web` |
| Memory budgets | `cargo run -p mareader-ui --example memory -- BOOK` | `python3 tools/ci.py memory --ref BRANCH` |
| Kokoro parity | none (needs the 650 MB model and builds) | `python3 tools/ci.py kokoro --ref BRANCH` |
| Piper parity | none (needs six voices and onnxruntime) | `python3 tools/ci.py piper --ref BRANCH` |
| Kitten parity | none (needs four builds and onnxruntime) | `python3 tools/ci.py kitten --ref BRANCH` |
| Other languages' phonemes | none (needs espeak-ng) | `python3 tools/ci.py ipa --ref BRANCH` |
| Installers | `cargo packager --release`, then `tools/check-installers.sh target/release` | `python3 tools/ci.py installers` |
| Polls CI | no | yes, until the verdict |

## Why two modes

A capable workstation builds the workspace in minutes and keeps its cache.
A sandbox does neither. The one this repository is developed in has 2 GB of
RAM and 2 cores, and its build cache is lost at every reset. Measured there
on 2026-10-14:

| | Time |
|---|---|
| Cold build of the gates, after a reset (one compiler at a time) | 30–35 min |
| CI, push to a green verdict (six jobs, warm GitHub cache) | 2 min |
| CI, push to a red format job, digest included | 28 s |
| `tools/ci.py shots`, dispatch to PNGs on disk | 70–85 s |
| `tools/ci.py web`, dispatch to the preview on disk (cold wasm build) | 2.5 min |
| `tools/ci.py cache restore` (1.7 GB at 8–17 MB/s, then unpack) | 3.5 min |
| The full local gates right after a restore (nothing compiles) | 11 s |
| `tools/ci.py cache build`, seeded from the previous cache | 2.5 min, plus changed crates |
| `tools/ci.py cache build --cold`, from nothing | 13.5 min |

So on such a machine the gates belong in CI, and on a workstation polling CI
is a waste: the same checks already passed locally.

## Choosing a mode

Highest precedence first:

1. **The user's words.** "Use CI", "use the workflow" or "don't build
   locally" mean CI mode, even on a fast machine. "Build locally" or "run the
   tests here" mean local mode, even on a slow one. The choice stands for the
   session, or until the user changes it.
2. **`MAREADER_VERIFY=local` or `MAREADER_VERIFY=ci`** in the environment: a
   standing preference for a machine.
3. **Detection:** `python3 tools/ci.py mode` prints a recommendation and its
   reasons. It picks local mode with at least 8 GiB of RAM (or cgroup limit),
   4 cores and 10 GiB of free disk; otherwise CI mode.

State the mode and the reason in the first report of a session, so the user
can overrule it, e.g. "CI mode: 1.9 GiB RAM, 2 cores".

On a low-end machine, local mode still works; it is just slow. The first
build takes about half an hour. Run every cargo command through the
machine's memory cap, one compiler at a time. In the sandbox, a restored
cache cuts that first build down (see "The sandbox cache" below).

## Local mode

- Run the gates in AGENTS.md before each commit, and take shots with the
  `shot` example.
- Push when green. **Do not poll CI.** It runs the same checks, so its answer
  is almost always green, and waiting on it wastes the round.
- CI also builds on macOS and Windows, which a Linux machine cannot run
  locally. Glance at it with `python3 tools/ci.py watch --once` at the start
  of the next session, or before a release; a red result there is a real
  bug.

## CI mode

The loop, per change:

1. **Run the cheap checks locally.** They compile nothing:
   - `python3 tools/comment_check.py <files>`;
   - `python3 tools/commit_check.py --range origin/main..HEAD`;
   - `cargo fmt --all --check`, if a toolchain is installed (rustfmt only
     parses).
2. **Commit on a `<type>/<slug>` branch and push it.** CI runs on pushes to
   those branches (`feat/`, `fix/`, `perf/`, `refactor/`, `docs/`, `test/`,
   `build/`, `ci/`, `chore/`), so `main` only ever takes green commits.
3. **Watch the verdict:** run `python3 tools/ci.py watch`. It stops at the
   first red job and prints the failing step: the command, its output and
   the error. Exit code 0 means green, 1 red, 2 still running. On a long
   wait, use `--detach` and read progress later with `watch --tail`; stop it
   with `watch --stop`.
4. **Red:** fix it in a new commit, push, and watch again. Never re-run a
   green run, and never re-run a red one hoping it turns green.
5. **Green:** merge into `main`, either by fast-forward
   (`git push origin HEAD:main`) or through a PR, then delete the branch. CI
   runs again on `main` for the same commit; there is no need to wait for it.
6. **Look at the result:** run `python3 tools/ci.py shots` for any visible
   change, at the four zooms ("Review every update").

What CI covers, against the local gates:

| Gate | CI job |
|---|---|
| `cargo fmt --all --check` | Rust / format |
| `cargo clippy --workspace --all-targets` | Rust / lint |
| clippy on macOS, with and without default features (native: bundled SQLite needs the SDK) | Build / macos-latest |
| clippy for `wasm32-unknown-unknown` | none: the browser preview is best-effort, with no CI job |
| `cargo test --workspace` (with lavapipe for GPU tests) | Rust / test |
| `cargo xtask layers`, `parity`, `assets`, `unsafe` | Rust / test |
| every commit builds alone (`--locked`) | Build / each commit |
| native macOS and Windows builds | Build / macos-latest, windows-latest |

**Hybrid.** In CI mode on a machine whose cache is warm, one targeted local
command is fine for a tight loop (for example
`cargo test -p mareader-ui some_test`, under the memory cap). The gates
still come from CI.

**Superseded runs.** A newer push to the same branch cancels the older run.
`watch` reports that run as superseded, not red; watch the branch head
instead.

## Screenshots from CI

```sh
python3 tools/ci.py shots \
  "grid pdf=sample:20 workers=0 texture=grid gap=false scroll=900" \
  "dark pdf=sample:20 workers=0 theme=dark panes=2"
```

- **Format.** Each argument is one shot: a file name, then the `shot`
  example's `key=value` options (see `crates/ui/examples/shot.rs`).
  `workers=0` renders pages synchronously, so page content appears in the
  shot; without it, the shot may show blank pages.
- **Any setting (2026-10-22).** `set=<dotted.path>:<json>` writes one
  settings field through `Settings::set_path`, then sanitizes, for
  example `set=layout.progress_bar:false`. Every switch can be
  shot on and off without a new key; an unknown path fails the shot.
- **What happens.** The `Remote` workflow renders on lavapipe, the same
  software GPU the tests use. It reuses the test job's build cache and
  uploads only the PNGs, kept for 3 days.
- **Where they land.** In `target/remote/shots/<name>.png`; open them with
  `read_file`.
- **The commit must be pushed.** Shots render a pushed commit (`--rev`,
  default `HEAD`), so push the work branch first. Uncommitted edits are not
  in the shot.

This beats building the browser preview and screenshotting it in a local
Chromium. The native renderer is the product; a wasm build here costs a
second cold build for another target, a wasm-bindgen install, and a
~150 MB browser plus its system libraries.

## The browser preview from CI

`python3 tools/ci.py web` builds `tools/web.sh`'s output in CI, with the
wasm-bindgen version that `Cargo.lock` pins. It also tries a headless-Chrome
screenshot, `preview.png`; that part is best-effort. Everything lands in
`target/remote/web/` (or `--out DIR`). To show the user the live preview:

```sh
python3 -m http.server -d target/remote/web 8000 --bind 0.0.0.0
```

Use it only to check the browser preview itself. Desktop changes are
checked with shots.

## Memory from CI

`python3 tools/ci.py memory --ref BRANCH --rev SHA` runs the Remote memory
task: two PDFs and two EPUBs read in rounds, a 3,000-PDF library and 20,000
books without files, then the EPUB layout test. It takes about 15 minutes.

- **Where it lands.** `target/remote/memory/memory.md` (each workload's
  rows and budgets) and `layout.txt`; the run's summary shows the same.
- **The verdict.** The command returns 1 when any budget reads `**no**`.
  Piped through `sed` or `grep`, that status is lost, so read the file.
- **What the numbers mean.** `docs/memory.md`, including why RSS on CI's
  software GPU counts what a real GPU keeps in VRAM.

## Kokoro parity from CI

`python3 tools/ci.py kokoro --ref BRANCH --rev SHA` runs `tools/kokoro-parity`,
which loads the pinned Kokoro model twice: once through `mareader-kokoro`, once
through the reference Rust build it was ported from. Both get the same phonemes,
voice and frame timing.

- **Activation.** The reference runs ALBERT's feed-forward with exact GELU.
  PyTorch's Kokoro, left at ALBERT's default, and Kitten's exports use its
  tanh form, as the app does; here ours is set to exact GELU to compare alike.
- **Exact stages.** Phoneme encoder, projection, duration encoder, raw and
  rounded durations, pitch and energy curves, text encoder. Each is "same"
  under 1e-3 relative error, "close" under 1e-2, else it fails.
- **Audio.** The noise source is seeded here but random there, so waveforms
  can't match sample for sample. Takes must agree in length (within 2%), RMS
  (within 0.9 to 1.1 times), 10 ms loudness curve and waveform (correlation
  above 0.98 each; two reference runs reach about 0.994). This holds for the
  decoder alone and for whole texts, chunks and pauses included.
- **Generator steps.** The tool carries a copy of the reference decoder that
  draws noise as ours does. Fed the same seed, every generator step is listed
  side by side, and the final audio must correlate above 0.99. Noise
  diagnostics show how far reseeding or silencing the noise moves each take.
- **G2P.** `mareader-g2p` against `loqui-g2p` 0.4.0, the crate it ports, in
  both dialects: prose, tricky sentences, thousands of numbers, years, money,
  a third of each lexicon with cased and suffixed forms, and made-up words for
  the neural model. Every input must give the same phonemes and tokens. The
  G2P files download with the model; `kokoro-parity <dir> <out> g2p` runs
  this part alone.
- **Builds.** Each smaller build (16-bit and 8-bit GGUF) speaks the same
  phonemes as the full weights, noise seeded alike. Rounded durations can
  shift a frame, so the gates are loudness curve, length and RMS: Half above
  0.98 within 2% and 10%, Small above 0.95 within 5% and 15%. Waveform
  correlation and matching durations are listed too.
  `kokoro-parity <dir> <out> builds` runs this with the G2P part.
- **Where it lands.** `target/remote/kokoro/summary.md`, plus each take as
  WAV to listen to. Speed rows give real-time factors on CI's CPU.
- **The model.** Fetched from its pinned revision, checked by SHA-256 and kept
  in the Actions cache.

## Piper parity from CI

`python3 tools/ci.py piper --ref BRANCH --rev SHA` runs `tools/piper-parity`
on six pinned voices: low, medium and high quality, American and British, one
with 109 speakers. Our G2P writes each sentence's phonemes and ids once; both
sides speak those same ids.

- **Noiseless.** With both noise scales at 0 the model is deterministic, so
  `mareader-vits` must match onnxruntime running the voice's own ONNX file:
  the same length to the sample (in at least 80% of sentences, since a
  duration can round across a frame) and waveform correlation of 0.999 or more.
- **With noise.** The noise differs, so totals are compared: length within
  15% and RMS within 25% of the reference.
- **Phonemes.** Our G2P's phonemes, respelled for the voice's espeak style,
  against espeak's own for the same text; the error rate must stay under 25%.
  espeak and onnxruntime come from `piper-tts` in this job only; neither is
  linked into or shipped with the app.
- **Where it lands.** `target/remote/piper/summary.md`, with load time and
  real-time factors for both sides, and the first takes as WAV.
- **The voices.** Fetched from their pinned revision, checked by SHA-256 and
  kept in the Actions cache.
- **The engine.** The app's own Piper engine then speaks two sentences from
  two voices laid out as downloads leave them, and the summary gains when the
  first sound was ready. The kokoro task does the same for Kokoro.

## Kitten parity from CI

`python3 tools/ci.py kitten --ref BRANCH --rev SHA` runs `tools/kitten-parity`
on Kitten's four pinned builds: Nano in full precision and 8-bit, Micro and
Mini. Our G2P writes ten sentences' phonemes and ids once, the voice rotating
through all eight; both sides speak those same ids at speed 1.

- **Seeded alike.** The reference graph's random nodes are swapped for our
  seeded noise and fixed phases, so `mareader-kokoro` must match onnxruntime
  on full-precision Nano: equal durations and length in at least 80% of
  sentences and waveform correlation of 0.99 or more. Silenced noise would not
  do: unvoiced stretches then turn constant, and their spectrum's empty bins
  get phases from rounding alone.
- **Weights by name.** Every build must yield each weight full-precision Nano
  does, since a missing norm or bias would otherwise load as its default. An
  export that merged identical norms drops their nodes; those count apart, as
  Mini's norms are all 1 and 0.
- **Every build.** Total length within 10% and RMS within 25%. The 8-bit
  builds round activations as onnxruntime runs them, so their waves differ.
- **Phonemes.** Ours against espeak's, as for Piper, under 25% error. espeak
  and onnxruntime are installed in this job only.
- **Where it lands.** `target/remote/kitten/summary.md`, with the sizes read
  from each file, load time, real-time factors and the first takes as WAV.
- **The engine.** The app's own Kitten engine then speaks two sentences from
  a Nano build laid out as downloads leave it.

## Other languages' phonemes from CI

`python3 tools/ci.py ipa --ref BRANCH --rev SHA` builds `mareader-ipa`'s
`ipa` example and runs `tools/ipa-parity/check.py`: 500 public-domain
sentences a language, our phonemes against espeak-ng's, which Kokoro's Spanish,
Italian, Portuguese and French voices were trained on.

- **As the voices read them.** espeak's side takes misaki's steps for Kokoro,
  ties written as single letters; both sides keep only Kokoro's vocabulary.
- **The gate.** Phoneme error at most 0.5% for Spanish, whose spelling gives
  its sounds and stress, and 5% for Italian, where stress and open vowels are
  partly a matter of the word. Portuguese and French may err 2%: open vowels
  and links between words follow the word there too.
- **Where it lands.** `target/remote/ipa/summary.md`, with the most frequent
  words that differ. espeak-ng and phonemizer are installed in this job only.

## The sandbox cache

After a sandbox reset, local mode would start with a 30-minute cold build.
`python3 tools/ci.py cache restore` instead downloads a build that CI made of
everything the local gates build, toolchain included, and unpacks it here.
`python3 tools/ci.py cache build` refreshes that build.

**Use it only when it is needed.** Each step costs minutes:

- **Restore** only on a cold machine (after a reset) when local work is
  wanted: local mode, or CI mode's hybrid loop. In plain CI mode nothing
  compiles locally, so skip it. A warm machine never needs it.
- **Build** only when `restore` reports no live cache, or `Cargo.lock`
  changed a lot since the cache was made. An older cache is still worth
  restoring: dependencies are reused, and only crates edited since then
  recompile (seconds to a couple of minutes). Never build "just to be
  fresh".
- **`--cold`** only to move to a new toolchain or to drop accumulated
  leftovers. The cache ships its own compiler, so it keeps the one it was
  first built with until then.

- **What is inside.** The `rustup` toolchain (with the wasm and macOS
  targets, clippy and rustfmt), the cargo registry and the full
  `target-mr` directory, packed with zstd. That is about 1.7 GB to
  download and 7 GB unpacked; the artifact is kept for 30 days.
- **Why a build takes minutes, not 13.** Normal CI jobs start from
  GitHub's saved build and only compile our crates. That saved build sits
  at other paths with other settings, so cargo cannot reuse it here. A
  cold cache build therefore compiles all ~300 crates on one machine. A
  normal cache build instead downloads the previous cache inside GitHub
  (100 MB/s, about 30 s), compiles only what changed, and repacks.
- **Why it can be reused.** Cargo keys its cache on absolute paths, the
  compiler version and the build profile. So the CI job clones the repo to
  `/home/user/mareader2` and uses `/home/user/.cache/{rustup,cargo,target-mr}`
  with `capped.sh`'s profile (`debug = 0`). It builds with the toolchain it
  then ships.
- **Incremental compilation stays on.** It is part of the profile hash, and
  the runner turns it off (`CARGO_INCREMENTAL=0`). With it off, every
  workspace crate rebuilt after a restore, though no dependency did. The
  job sets `CARGO_INCREMENTAL=1` to match cargo's default here.
- **Why nothing recompiles.** Cargo also compares file times. The cache
  records every tracked file's time. `restore`, and the next cache build's
  seed step, give each file that is unchanged since the cached commit its
  recorded time back. So only crates edited since then recompile.
- **When to use it.** A fresh sandbox where the user wants local mode, or
  CI mode with a warm cache for the hybrid loop. Rebuild the cache after
  `Cargo.lock` or the toolchain changes. A stale cache still works: only
  what changed recompiles.
- **Limits.**
  - It fits only a machine with this exact layout (x86_64 Linux, the paths
    above); anywhere else, cargo would rebuild from scratch.
  - It refuses to overwrite an existing cache unless you pass `--force`.
  - It needs about 4× the archive size in free disk while unpacking.
  - It installs `zstd` with `sudo apt-get` if it is missing.

GitHub's own build caches (`actions/cache`, `Swatinem/rust-cache`) cannot
be downloaded outside a runner; `gh` has no command for it. That is why the
cache travels as a workflow artifact.

## The token

- **Reads need no token.** This repository is public, so `watch` and
  `mode` work without one, at 60 API requests an hour.
- **Other commands need one.** Dispatching (`shots`, `web`, `cache build`)
  and downloading artifacts need a token with Actions read and write and
  Contents read.
- **How to pass it.** Set `$GITHUB_TOKEN` or `$GH_TOKEN`, or pass
  `--token-file PATH` before the subcommand. The tool never writes the token
  anywhere. Never commit it, and mask it in any output you paste.

## Troubleshooting

- **"is not on GitHub: push it first."** Shots, web and cache work from
  pushed commits only. Push the work branch first.
- **"no workflow runs appeared."** The push may have touched only `docs/`,
  which CI ignores, or the branch name may not match a `<type>/` prefix.
- **HTTP 403 when dispatching.** The token lacks Actions write, or it has
  expired.
- **Restore says files "already exist."** The machine is already warm. Pass
  `--force` only if the cache there is broken.
- **Crates recompile after a restore.** Run `cargo build -v`. A "Dirty"
  line names a stale file or setting. A crate compiling with no "Dirty"
  line has a new hash: compare its JSON under `target-mr/debug/.fingerprint/`
  with the restored one. A differing `profile` field means a profile
  setting differs, as `CARGO_INCREMENTAL` once did. If all dependencies
  recompile instead, the paths or the toolchain differ; `restore` prints
  the cached `rustc`.

## What only a real machine can check

Headless tests prove Mareader asks for the right thing: a caption click,
a drag on the empty bar, a press on a frameless edge each send their
window command (`crates/ui/tests/chrome.rs`). Whether the system then
moves, resizes or closes the window is eframe's and the OS's, so the
owner checks these on real machines before 1.0:

| Check | Where | How |
| --- | --- | --- |
| Drag the empty title bar; double-click it | every OS | the window follows; it maximises and restores |
| Resize from each edge and corner | Windows; Linux on Wayland and X11 | the cursor changes, the window resizes |
| Caption buttons | Windows; GNOME | minimise, maximise and restore, close |
| Traffic lights | macOS | sit in the bar at x 20, slide with the sidebar |
| Open from the file manager | every OS, installed | the book opens in the running window |

**2026-10-17, macOS:** the owner ran the Mac rows on a real Mac with an
installed build: the bar drags and double-clicks, the traffic lights sit
in the bar and slide with the sidebar, and books open from Finder into
the running window. All passed. Each installer's launch is also checked
in CI on every OS (`docs/packaging.md` § Installers from CI).

