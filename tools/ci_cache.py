"""The sandbox cache (docs/verification.md): CI packs a build, a sandbox restores it.

tools/ci.py loads this module; run that, not this file.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import time
import zipfile

from ci import (  # noqa: E402 - ci.py registers itself before importing this
    api,
    artifact,
    dispatch,
    download,
    follow,
    git,
    log,
    pushed_sha,
    token_from,
)


def cache_build(args) -> int:
    token = token_from(args)
    sha = pushed_sha(args.repo, token, args.rev)
    cold = "true" if args.cold else "false"
    full = "true" if args.full else "false"
    run = follow(args.repo, token,
                 dispatch(args.repo, token, "cache", sha, cold=cold, full=full),
                 args.timeout, args.poll)
    return 0 if run.conclusion == "success" else 1


def newest_cache(repo: str, token: str) -> dict | None:
    """The newest unexpired sandbox cache artifact, if any."""
    payload = api(token, f"/repos/{repo}/actions/artifacts?name=sandbox-cache&per_page=20")
    live = [a for a in (payload or {}).get("artifacts", []) if not a["expired"]]
    return max(live, key=lambda a: a["created_at"]) if live else None


def ensure_zstd() -> None:
    if shutil.which("zstd"):
        return
    log("  installing zstd")
    for cmd in (["sudo", "-n", "apt-get", "install", "-y", "-q", "zstd"],
                ["sudo", "-n", "sh", "-c", "apt-get update -q && apt-get install -y -q zstd"]):
        if subprocess.run(cmd, capture_output=True).returncode == 0 and shutil.which("zstd"):
            return
    raise SystemExit("zstd is missing and could not be installed; install it and retry")


# Commits past which a restored cache recompiles most workspace crates.
STALE = 30


def cache_restore(args) -> int:
    token = token_from(args)
    if not token:
        raise SystemExit("downloading needs a token: --token-file or $GITHUB_TOKEN")
    started = time.time()
    if args.run:
        art = artifact(args.repo, token, args.run, "sandbox-cache")
    else:
        art = newest_cache(args.repo, token)
        if art is None:
            raise SystemExit("no live sandbox cache; make one: python3 tools/ci.py cache build")
    meta = fetch_cache(token, art, "/home/user/.cache", args.force)
    if meta["repo_dir"] != git("rev-parse", "--show-toplevel"):
        log(f"  warning: the cache was built at {meta['repo_dir']}; cargo will rebuild")
    log(f"restored in {time.time() - started:.0f}s; workspace crates edited since "
        f"{meta['sha'][:9]} recompile, dependencies do not")
    try:
        behind = int(git("rev-list", "--count", f"{meta['sha']}..HEAD"))
    except (subprocess.CalledProcessError, ValueError):
        behind = -1
    # Past this, most crates recompile: a 12-minute first lint here.
    if behind > STALE or behind < 0:
        log(f"  stale: {behind if behind >= 0 else '?'} commits behind HEAD. Lint in "
            "CI (about 1 min) rather than here; a green push refreshes the cache")
    return 0


def fetch_cache(token: str, art: dict, root: str, force: bool) -> dict:
    """Download a sandbox cache, unpack it, and redate the sources."""
    log(f"== sandbox cache {art['id']} from {art['created_at']}, "
        f"{art['size_in_bytes'] >> 20} MB")
    os.makedirs(os.path.join(root, "tmp"), exist_ok=True)
    zip_path = os.path.join(root, "tmp", "sandbox-cache.zip")
    need = art["size_in_bytes"] * 4
    if shutil.disk_usage(root).free < need:
        raise SystemExit(f"not enough disk: need ~{need >> 30} GB free under {root}")
    ensure_zstd()
    download(token, art, zip_path)
    with zipfile.ZipFile(zip_path) as zf:
        meta = json.loads(zf.read("meta.json"))
        root = meta["root"]
        present = [d for d in meta["dirs"] if os.path.exists(os.path.join(root, d))]
        if present and not force:
            raise SystemExit(f"{', '.join(present)} already exist under {root}: this "
                             "machine is warm already; --force replaces them")
        for d in present:
            shutil.rmtree(os.path.join(root, d))
        log(f"  unpacking into {root} ({meta['rustc']})")
        tar = subprocess.Popen(["tar", "--zstd", "-x", "-C", root, "-f", "-"],
                               stdin=subprocess.PIPE)
        with zf.open("sandbox-cache.tar.zst") as member:
            shutil.copyfileobj(member, tar.stdin, 1 << 20)
        tar.stdin.close()
        if tar.wait() != 0:
            raise SystemExit("tar failed; the cache is incomplete, delete it and retry")
    os.unlink(zip_path)
    log(f"  {redate(meta)} unchanged source files dated as at {meta['sha'][:9]}")
    return meta


def cache_seed(args) -> int:
    """In the cache job: start from the newest cache, not from nothing.
    Never fails the job; a cold build is the fallback."""
    token = token_from(args)
    try:
        art = newest_cache(args.repo, token)
        if art is None:
            log("no live sandbox cache: building cold")
            return 0
        meta = fetch_cache(token, art, "/home/user/.cache", force=True)
        # Nothing deletes a stale artifact: a lock or build-set change
        # would carry the old files along, so target-mr starts afresh.
        lock = hashlib.sha256(open("Cargo.lock", "rb").read()).hexdigest()
        builds = os.environ.get("CACHE_BUILDS", "lint")
        if meta.get("lock") != lock or meta.get("builds") != builds:
            log(f"Cargo.lock or the build set ({builds}) changed since that "
                "cache: target-mr builds from nothing; toolchain and registry stay")
            shutil.rmtree("/home/user/.cache/target-mr", ignore_errors=True)
    except (SystemExit, OSError, zipfile.BadZipFile) as exc:
        log(f"seeding failed ({exc}): building cold")
        for d in ("rustup", "cargo", "target-mr"):
            shutil.rmtree(os.path.join("/home/user/.cache", d), ignore_errors=True)
    return 0


def redate(meta: dict) -> int:
    """Give unchanged sources the times CI built them with, so cargo
    rebuilds only what differs from the cached commit."""
    sha = meta["sha"]
    if subprocess.run(["git", "cat-file", "-e", f"{sha}^{{commit}}"],
                      capture_output=True).returncode != 0:
        subprocess.run(["git", "fetch", "-q", "origin", sha], capture_output=True)
    try:
        changed = set(git("diff", "--name-only", sha).splitlines())
    except subprocess.CalledProcessError:
        log("  cached commit unknown here: every crate recompiles")
        return 0
    # Per-file times since seeding; older caches dated all to the commit.
    times = meta.get("mtimes", {})
    count = 0
    for path in git("ls-files").splitlines():
        if path not in changed and os.path.isfile(path) and not os.path.islink(path):
            t = times.get(path, meta["time"])
            os.utime(path, (t, t))
            count += 1
    return count
