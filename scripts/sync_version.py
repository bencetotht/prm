#!/usr/bin/env python3

from __future__ import annotations

import argparse
import pathlib
import re
import sys


README_DOWNLOAD_START = "<!-- release-download-example:start -->"
README_DOWNLOAD_END = "<!-- release-download-example:end -->"
README_ASSETS_START = "<!-- release-assets:start -->"
README_ASSETS_END = "<!-- release-assets:end -->"

TARGETS = [
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Sync or verify the release version across repo files."
    )
    parser.add_argument("version", help="Release version in X.Y.Z or vX.Y.Z form.")
    parser.add_argument(
        "--root",
        type=pathlib.Path,
        default=pathlib.Path(__file__).resolve().parents[1],
        help="Repository root to update. Defaults to the current repository.",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Verify that tracked versioned files already match the requested version.",
    )
    return parser.parse_args()


def normalize_version(raw: str) -> str:
    match = re.fullmatch(r"v?(\d+\.\d+\.\d+)", raw.strip())
    if not match:
        raise ValueError(
            f"invalid version {raw!r}; expected X.Y.Z or vX.Y.Z"
        )
    return match.group(1)


def replace_readme_block(
    text: str, start_marker: str, end_marker: str, content: str
) -> str:
    pattern = re.compile(
        rf"({re.escape(start_marker)}\n)(.*?)(\n{re.escape(end_marker)})",
        re.DOTALL,
    )
    updated, count = pattern.subn(rf"\1{content}\3", text, count=1)
    if count != 1:
        raise ValueError(f"missing README marker pair: {start_marker} / {end_marker}")
    return updated


def update_cargo_toml(text: str, version: str) -> str:
    lines = text.splitlines(keepends=True)
    in_package = False
    replaced = False

    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped == "[package]":
            in_package = True
            continue
        if stripped.startswith("[") and stripped != "[package]":
            in_package = False
        if in_package and line.startswith("version = "):
            lines[index] = f'version = "{version}"\n'
            replaced = True
            break

    if not replaced:
        raise ValueError("could not find package.version in Cargo.toml")

    return "".join(lines)


def update_cargo_lock(text: str, version: str) -> str:
    lines = text.splitlines(keepends=True)
    in_package = False
    package_name = None

    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped == "[[package]]":
            in_package = True
            package_name = None
            continue
        if in_package and stripped.startswith("name = "):
            package_name = stripped.removeprefix('name = "').removesuffix('"')
            continue
        if in_package and stripped.startswith("version = ") and package_name == "prm":
            lines[index] = f'version = "{version}"\n'
            return "".join(lines)
        if stripped.startswith("[[package]]") or stripped.startswith("[metadata]"):
            in_package = stripped == "[[package]]"

    raise ValueError('could not find [[package]] entry for "prm" in Cargo.lock')


def download_example(version: str) -> str:
    return "\n".join(
        [
            "```bash",
            f"curl -fsSL https://github.com/bencetotht/prm/releases/download/v{version}/prm-v{version}-aarch64-apple-darwin.tar.gz -o prm.tar.gz",
            "tar -xzf prm.tar.gz",
            f'install "./prm-{version}-aarch64-apple-darwin/prm" /usr/local/bin/prm',
            "```",
        ]
    )


def release_assets(version: str) -> str:
    lines = [
        f"- `prm-v{version}-{target}.tar.gz`"
        for target in TARGETS
    ]
    lines.append(f"- `prm-v{version}-checksums.txt`")
    return "\n".join(lines)


def update_readme(text: str, version: str) -> str:
    updated = replace_readme_block(
        text, README_DOWNLOAD_START, README_DOWNLOAD_END, download_example(version)
    )
    return replace_readme_block(
        updated, README_ASSETS_START, README_ASSETS_END, release_assets(version)
    )


def sync_targets(root: pathlib.Path, version: str) -> list[tuple[pathlib.Path, str]]:
    cargo_toml = root / "Cargo.toml"
    cargo_lock = root / "Cargo.lock"
    readme = root / "README.md"

    return [
        (cargo_toml, update_cargo_toml(cargo_toml.read_text(), version)),
        (cargo_lock, update_cargo_lock(cargo_lock.read_text(), version)),
        (readme, update_readme(readme.read_text(), version)),
    ]


def main() -> int:
    args = parse_args()

    try:
        version = normalize_version(args.version)
        updates = sync_targets(args.root.resolve(), version)
    except ValueError as err:
        print(f"error: {err}", file=sys.stderr)
        return 1

    stale_paths = [
        path for path, updated_content in updates if path.read_text() != updated_content
    ]

    if args.check:
        if stale_paths:
            for path in stale_paths:
                print(f"unsynced: {path}", file=sys.stderr)
            return 1
        print(f"version files already synced to {version}")
        return 0

    for path, updated_content in updates:
        path.write_text(updated_content)
        print(f"synced: {path}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
