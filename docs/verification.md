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
| `tools/ci.py cache restore` (1.7 GB at 8–17 MB/s, then unpack) | 3.7 min |
| The full local gates right after a restore (nothing compiles) | 11 s |
| `tools/ci.py cache build`, in CI | 13.5 min |

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
| clippy for `aarch64-apple-darwin`, with and without default features | Rust / lint |
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

## The sandbox cache

After a sandbox reset, local mode would start with a 30-minute cold build.
`python3 tools/ci.py cache build` has CI build everything the local gates
build. `python3 tools/ci.py cache restore` downloads that build and unpacks
it here, toolchain included.

- **What is inside.** The `rustup` toolchain (with the wasm and macOS
  targets, clippy and rustfmt), the cargo registry and the full
  `target-mr` directory, packed with zstd. That is about 1.7 GB to
  download and 7 GB unpacked; the artifact is kept for 7 days.
- **Why it can be reused.** Cargo keys its cache on absolute paths, the
  compiler version and the build profile. So the CI job clones the repo to
  `/home/user/mareader2` and uses `/home/user/.cache/{rustup,cargo,target-mr}`
  with `capped.sh`'s profile (`debug = 0`). It builds with the toolchain it
  then ships.
- **Incremental compilation stays on.** It is part of the profile hash, and
  the runner turns it off (`CARGO_INCREMENTAL=0`). With it off, every
  workspace crate rebuilt after a restore, though no dependency did. The
  job sets `CARGO_INCREMENTAL=1` to match cargo's default here.
- **Why nothing recompiles.** Cargo also compares file times. CI dates every
  source file to its commit time before building. `restore` gives every file
  that is unchanged since that commit the same date, so only crates you have
  edited since recompile.
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
