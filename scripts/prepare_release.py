#!/usr/bin/env python3

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys


VERSIONED_FILES = ("Cargo.toml", "Cargo.lock", "README.md")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Prepare a local release commit and annotated tag."
    )
    parser.add_argument("version", help="Release version in X.Y.Z or vX.Y.Z form.")
    parser.add_argument(
        "--root",
        type=pathlib.Path,
        default=pathlib.Path(__file__).resolve().parents[1],
        help="Repository root to update. Defaults to the current repository.",
    )
    parser.add_argument(
        "--check-only",
        action="store_true",
        help="Verify the repository is ready for the requested release without editing or tagging.",
    )
    return parser.parse_args()


def normalize_version(raw: str) -> str:
    match = re.fullmatch(r"v?(\d+\.\d+\.\d+)", raw.strip())
    if not match:
        raise ValueError(f"invalid version {raw!r}; expected X.Y.Z or vX.Y.Z")
    return match.group(1)


def git(root: pathlib.Path, *args: str, capture_output: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=root,
        check=False,
        text=True,
        capture_output=capture_output,
    )


def ensure_git_repo(root: pathlib.Path) -> None:
    result = git(root, "rev-parse", "--show-toplevel", capture_output=True)
    if result.returncode != 0:
        raise RuntimeError("repository root is not inside a git worktree")


def ensure_clean_worktree(root: pathlib.Path) -> None:
    status = git(root, "status", "--short", capture_output=True)
    if status.returncode != 0:
        raise RuntimeError(status.stderr.strip() or "failed to inspect git status")
    if status.stdout.strip():
        raise RuntimeError(
            "git worktree is not clean; commit or stash changes before preparing a release"
        )


def ensure_tag_absent(root: pathlib.Path, tag: str) -> None:
    result = git(root, "rev-parse", "--verify", "--quiet", f"refs/tags/{tag}")
    if result.returncode == 0:
        raise RuntimeError(f"tag {tag} already exists")


def run_sync(root: pathlib.Path, version: str, check_only: bool) -> None:
    script = pathlib.Path(__file__).with_name("sync_version.py")
    command = [sys.executable, str(script), version, "--root", str(root)]
    if check_only:
        command.append("--check")

    result = subprocess.run(command, check=False, text=True)
    if result.returncode != 0:
        mode = "verify" if check_only else "sync"
        raise RuntimeError(f"failed to {mode} versioned release files")


def commit_release(root: pathlib.Path, tag: str) -> None:
    add_result = git(root, "add", *VERSIONED_FILES)
    if add_result.returncode != 0:
        raise RuntimeError("failed to stage versioned release files")

    diff_result = git(root, "diff", "--cached", "--quiet")
    if diff_result.returncode == 0:
        raise RuntimeError(
            f"versioned files already match {tag}; refusing to create an empty release commit"
        )
    if diff_result.returncode not in (0, 1):
        raise RuntimeError("failed to inspect staged release changes")

    commit_result = git(root, "commit", "-m", f"release: {tag}")
    if commit_result.returncode != 0:
        raise RuntimeError("failed to create release commit")


def create_tag(root: pathlib.Path, tag: str) -> None:
    tag_result = git(root, "tag", "-a", tag, "-m", f"prm {tag}")
    if tag_result.returncode != 0:
        raise RuntimeError(f"failed to create annotated tag {tag}")


def main() -> int:
    args = parse_args()
    root = args.root.resolve()

    try:
        version = normalize_version(args.version)
        tag = f"v{version}"
        ensure_git_repo(root)
        ensure_clean_worktree(root)
        ensure_tag_absent(root, tag)

        if args.check_only:
            run_sync(root, tag, check_only=True)
            print(f"release prep check passed for {tag}")
            return 0

        run_sync(root, tag, check_only=False)
        run_sync(root, tag, check_only=True)
        commit_release(root, tag)
        create_tag(root, tag)
    except (RuntimeError, ValueError) as err:
        print(f"error: {err}", file=sys.stderr)
        return 1

    print(f"prepared release commit and annotated tag {tag}")
    print("next steps:")
    print("  git push origin HEAD")
    print(f"  git push origin {tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
