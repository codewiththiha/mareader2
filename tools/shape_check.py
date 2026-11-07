#!/usr/bin/env python3
"""Fail when Rust files add new size, function-length, or mod.rs logic debt."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BASELINE = ROOT / "tools" / "shape-baseline.json"
FILE_LIMIT = 500
FUNCTION_LIMIT = 60
SKIP_DIRS = {
    ".arena",
    ".cache",
    ".git",
    ".local",
    ".mypy_cache",
    ".next",
    ".nox",
    ".npm",
    ".nuxt",
    ".output",
    ".parcel-cache",
    ".playwright-cli",
    ".pytest_cache",
    ".ruff_cache",
    ".svelte-kit",
    ".tox",
    ".turbo",
    ".venv",
    ".vite",
    "__pycache__",
    "build",
    "coverage",
    "deliverables",
    "dist",
    "node_modules",
    "out",
    "target",
}
ALLOW_RE = re.compile(r"^\s*(?://!?|/\*|\*)\s*shape:\s*allow\s+(.+?)\s*(?:\*/)?$")


class Token:
    __slots__ = ("value", "line")

    def __init__(self, value: str, line: int) -> None:
        self.value = value
        self.line = line


def _newline_count(text: str) -> int:
    return text.count("\n")


def _raw_string_start(text: str, index: int) -> tuple[int, int] | None:
    """Return (opening quote index, hash count) for a Rust raw string."""
    if text.startswith(("br", "cr"), index):
        index += 1
    elif text[index] != "r":
        return None

    if index >= len(text) or text[index] != "r":
        return None
    index += 1
    hashes = 0
    while index < len(text) and text[index] == "#":
        hashes += 1
        index += 1
    if index < len(text) and text[index] == '"':
        return index, hashes
    return None


def _skip_quoted(text: str, index: int, line: int) -> tuple[int, int]:
    """Skip an ordinary Rust string, starting at its opening double quote."""
    index += 1
    while index < len(text):
        char = text[index]
        if char == "\\":
            if index + 1 < len(text) and text[index + 1] == "\n":
                line += 1
            index += 2
            continue
        if char == '"':
            return index + 1, line
        if char == "\n":
            line += 1
        index += 1
    return index, line


def _skip_char(text: str, index: int, line: int) -> tuple[int, int] | None:
    """Skip a Rust character literal; return None when this is a lifetime."""
    cursor = index + 1
    if cursor >= len(text) or text[cursor] in "\r\n":
        return None
    if text[cursor] == "\\":
        cursor += 1
        if cursor >= len(text):
            return None
        if text[cursor] == "u" and cursor + 1 < len(text) and text[cursor + 1] == "{":
            close = text.find("}", cursor + 2)
            if close < 0:
                return None
            cursor = close + 1
        else:
            cursor += 1
    else:
        cursor += 1
    if cursor < len(text) and text[cursor] == "'":
        return cursor + 1, line
    return None


def lex_rust(text: str) -> list[Token]:
    """A small lexer sufficient to locate function items outside comments/strings."""
    tokens: list[Token] = []
    index = 0
    line = 1
    length = len(text)
    while index < length:
        char = text[index]
        if char.isspace():
            if char == "\n":
                line += 1
            index += 1
            continue

        if text.startswith("//", index):
            end = text.find("\n", index + 2)
            if end < 0:
                break
            index = end
            continue

        if text.startswith("/*", index):
            depth = 1
            index += 2
            while index < length and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    if text[index] == "\n":
                        line += 1
                    index += 1
            continue

        raw = _raw_string_start(text, index)
        if raw is not None:
            quote, hashes = raw
            line += _newline_count(text[index : quote + 1])
            terminator = '"' + ("#" * hashes)
            end = text.find(terminator, quote + 1)
            if end < 0:
                break
            line += _newline_count(text[quote + 1 : end])
            index = end + len(terminator)
            continue

        # Byte/C strings share the same ordinary-string rules.
        quote_index = index
        if text.startswith(("b\"", "c\""), index):
            quote_index += 1
        if text[quote_index] == '"':
            index, line = _skip_quoted(text, quote_index, line)
            continue

        if char == "'":
            skipped = _skip_char(text, index, line)
            if skipped is not None:
                index, line = skipped
                continue

        # Raw identifiers (r#type) are identifiers, not keywords.
        if text.startswith("r#", index) and index + 2 < length and (
            text[index + 2] == "_" or text[index + 2].isalpha()
        ):
            end = index + 3
            while end < length and (text[end] == "_" or text[end].isalnum()):
                end += 1
            tokens.append(Token(text[index:end], line))
            index = end
            continue

        if char == "_" or char.isalpha() or ord(char) >= 128 and char.isidentifier():
            end = index + 1
            while end < length and (
                text[end] == "_" or text[end].isalnum() or ord(text[end]) >= 128
            ):
                end += 1
            tokens.append(Token(text[index:end], line))
            index = end
            continue

        # Keep delimiters separate; group other punctuation when it is common
        # Rust syntax so diagnostics and generic parsing remain predictable.
        two = text[index : index + 2]
        if two in {"::", "->", "=>", "&&", "||", ".."}:
            tokens.append(Token(two, line))
            index += 2
        else:
            tokens.append(Token(char, line))
            index += 1
    return tokens


def _matching(tokens: list[Token], opening: int, left: str, right: str) -> int | None:
    depth = 0
    for index in range(opening, len(tokens)):
        if tokens[index].value == left:
            depth += 1
        elif tokens[index].value == right:
            depth -= 1
            if depth == 0:
                return index
    return None


def function_spans(tokens: list[Token]) -> list[tuple[str, int, int]]:
    """Find named Rust function bodies; each result is (name, start, end)."""
    spans: list[tuple[str, int, int]] = []
    for index, token in enumerate(tokens):
        if token.value != "fn" or index + 1 >= len(tokens):
            continue
        name_token = tokens[index + 1]
        if not (name_token.value == "_" or name_token.value[0].isalpha() or name_token.value.startswith("r#")):
            continue
        name = name_token.value.removeprefix("r#")
        cursor = index + 2

        # Skip generic parameters, including braces in const generic defaults.
        if cursor < len(tokens) and tokens[cursor].value == "<":
            depth = 0
            while cursor < len(tokens):
                value = tokens[cursor].value
                if value == "<":
                    depth += 1
                elif value == ">":
                    depth -= 1
                    if depth == 0:
                        cursor += 1
                        break
                cursor += 1

        # Locate the parameter list, stopping at declarations without bodies.
        while cursor < len(tokens) and tokens[cursor].value not in {"(", ";", "{"}:
            cursor += 1
        if cursor >= len(tokens) or tokens[cursor].value != "(":
            continue
        params_end = _matching(tokens, cursor, "(", ")")
        if params_end is None:
            continue

        cursor = params_end + 1
        angle = bracket = paren = 0
        body_open: int | None = None
        while cursor < len(tokens):
            value = tokens[cursor].value
            if value == "<":
                angle += 1
            elif value == ">" and angle:
                angle -= 1
            elif value == "[":
                bracket += 1
            elif value == "]" and bracket:
                bracket -= 1
            elif value == "(":
                paren += 1
            elif value == ")" and paren:
                paren -= 1
            elif value == ";" and angle == bracket == paren == 0:
                break
            elif value == "{" and angle == bracket == paren == 0:
                body_open = cursor
                break
            cursor += 1
        if body_open is None:
            continue
        body_end = _matching(tokens, body_open, "{", "}")
        if body_end is not None:
            spans.append((name, token.line, tokens[body_end].line))
    return spans


def _allow_reason(text: str) -> str | None:
    for line in text.splitlines()[:10]:
        match = ALLOW_RE.match(line)
        if match:
            return match.group(1).strip()
    return None


def rust_files() -> list[Path]:
    found: list[Path] = []
    for directory, subdirs, files in os.walk(ROOT):
        subdirs[:] = sorted(name for name in subdirs if name not in SKIP_DIRS)
        for name in files:
            if name.endswith(".rs"):
                found.append(Path(directory) / name)
    return sorted(found)


def scan() -> dict[str, list[str]]:
    results: dict[str, list[str]] = {
        "large_files": [],
        "long_functions": [],
        "logic_modules": [],
    }
    baseline = _read_baseline(optional=True)
    for path in rust_files():
        relative = path.relative_to(ROOT).as_posix()
        text = path.read_text(encoding="utf-8")
        lines = text.splitlines()
        reason = _allow_reason(text)
        if reason is not None:
            if not reason:
                raise ValueError(f"{relative}: shape: allow needs a reason")
            # Keep existing exemptions in the baseline; a new exempt file is
            # intentionally absent from it and is reviewed by its annotation.
            for category, entries in baseline.items():
                results[category].extend(
                    entry
                    for entry in entries
                    if entry == relative or entry.startswith(relative + "::")
                )
            continue

        if len(lines) > FILE_LIMIT:
            results["large_files"].append(relative)
        spans = function_spans(lex_rust(text))
        for name, start, end in spans:
            if end - start + 1 > FUNCTION_LIMIT:
                results["long_functions"].append(f"{relative}::{name}@{start}")
        if path.name == "mod.rs" and spans:
            results["logic_modules"].append(relative)

    return {key: sorted(set(values)) for key, values in results.items()}


def _read_baseline(optional: bool = False) -> dict[str, list[str]]:
    if not BASELINE.exists():
        if optional:
            return {"large_files": [], "long_functions": [], "logic_modules": []}
        raise ValueError(f"missing baseline: {BASELINE.relative_to(ROOT)}")
    data = json.loads(BASELINE.read_text(encoding="utf-8"))
    expected = {"large_files", "long_functions", "logic_modules"}
    if not expected.issubset(data) or any(not isinstance(data[key], list) for key in expected):
        raise ValueError("shape baseline must contain large_files, long_functions, and logic_modules lists")
    if set(data) - expected - {"description"}:
        raise ValueError("shape baseline contains unknown categories")
    return {key: sorted(set(map(str, data[key]))) for key in expected}


def write_baseline(results: dict[str, list[str]]) -> None:
    document = {
        "description": "Existing Rust shape debt; remove entries as files/functions are brought under the limits.",
        **results,
    }
    BASELINE.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")


def check(results: dict[str, list[str]], baseline: dict[str, list[str]]) -> bool:
    failed = False
    for category in ("large_files", "long_functions", "logic_modules"):
        current = set(results[category])
        allowed = set(baseline[category])
        new = sorted(current - allowed)
        stale = sorted(allowed - current)
        if new:
            failed = True
            print(f"new {category.replace('_', ' ')}:")
            for item in new:
                print(f"  + {item}")
        if stale:
            failed = True
            print(f"remove resolved {category.replace('_', ' ')} from baseline:")
            for item in stale:
                print(f"  - {item}")
    if failed:
        print("shape gate failed; split the new debt or add a justified top-of-file `// shape: allow <reason>` exception")
        return False
    print(
        "shape gate passed: "
        f"{len(results['large_files'])} large files, "
        f"{len(results['long_functions'])} long functions, "
        f"{len(results['logic_modules'])} logic-bearing mod.rs files; no debt beyond baseline"
    )
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write-baseline",
        action="store_true",
        help="replace the checked-in baseline with the current shape inventory",
    )
    args = parser.parse_args()
    try:
        results = scan()
        if args.write_baseline:
            write_baseline(results)
            print(
                "wrote shape baseline: "
                f"{len(results['large_files'])} large files, "
                f"{len(results['long_functions'])} long functions, "
                f"{len(results['logic_modules'])} logic-bearing mod.rs files"
            )
            return 0
        return 0 if check(results, _read_baseline()) else 1
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"shape gate: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
