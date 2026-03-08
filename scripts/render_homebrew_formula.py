#!/usr/bin/env python3

from __future__ import annotations

import argparse
import pathlib
from string import Template


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render the Homebrew formula for a prm binary release."
    )
    parser.add_argument("--version", required=True, help="Release version in X.Y.Z form.")
    parser.add_argument("--github-owner", required=True)
    parser.add_argument("--github-repo", required=True)
    parser.add_argument("--template", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--linux-x86-64-sha256", required=True)
    parser.add_argument("--darwin-x86-64-sha256", required=True)
    parser.add_argument("--darwin-arm64-sha256", required=True)
    return parser.parse_args()


def release_url(owner: str, repo: str, version: str, target: str) -> str:
    tag = f"v{version}"
    asset = f"prm-v{version}-{target}.tar.gz"
    return f"https://github.com/{owner}/{repo}/releases/download/{tag}/{asset}"


def main() -> int:
    args = parse_args()
    template = Template(args.template.read_text())
    rendered = template.substitute(
        version=args.version,
        homepage=f"https://github.com/{args.github_owner}/{args.github_repo}",
        linux_x86_64_url=release_url(
            args.github_owner,
            args.github_repo,
            args.version,
            "x86_64-unknown-linux-gnu",
        ),
        linux_x86_64_sha256=args.linux_x86_64_sha256,
        darwin_x86_64_url=release_url(
            args.github_owner,
            args.github_repo,
            args.version,
            "x86_64-apple-darwin",
        ),
        darwin_x86_64_sha256=args.darwin_x86_64_sha256,
        darwin_arm64_url=release_url(
            args.github_owner,
            args.github_repo,
            args.version,
            "aarch64-apple-darwin",
        ),
        darwin_arm64_sha256=args.darwin_arm64_sha256,
    )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(rendered)
    print(f"rendered: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
