#!/usr/bin/env python3

from __future__ import annotations

import argparse
import pathlib
import tomllib
from string import Template


AUR_DESCRIPTION = "Terminal project repository manager."
LINUX_X86_64_ASSET = "linux-x86_64"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render the AUR PKGBUILD for a prm source release."
    )
    parser.add_argument("--pkgname", required=True, help="Final AUR package name.")
    parser.add_argument(
        "--variant",
        choices=("source", "bin"),
        default="source",
        help="Package variant to render.",
    )
    parser.add_argument("--template", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument(
        "--cargo-toml",
        type=pathlib.Path,
        default=pathlib.Path(__file__).resolve().parents[1] / "Cargo.toml",
        help="Path to the Cargo.toml to read package metadata from.",
    )
    return parser.parse_args()


def shell_quote(value: str) -> str:
    return "'" + value.replace("'", r"'\''") + "'"


def license_list(value: str) -> str:
    return " ".join(shell_quote(part.strip()) for part in value.split(" OR "))


def shell_array(values: list[str]) -> str:
    return "(" + " ".join(shell_quote(value) for value in values) + ")"


def main() -> int:
    args = parse_args()

    cargo = tomllib.loads(args.cargo_toml.read_text())
    package = cargo["package"]

    binary_name = package["name"]
    version = package["version"]
    repo_url = package["repository"].rstrip("/")
    source_dir = repo_url.rsplit("/", 1)[-1]
    pkgdesc = AUR_DESCRIPTION

    if args.variant == "source":
        conflicts = [f"{args.pkgname}-bin"]
        if args.pkgname != binary_name:
            conflicts.append(binary_name)
        provides: list[str] = []
        source_url = f"{repo_url}/archive/refs/tags/v{version}.tar.gz"
    else:
        if not args.pkgname.endswith("-bin"):
            raise SystemExit("error: binary variant package names must end with -bin")
        source_pkgname = args.pkgname.removesuffix("-bin")
        conflicts = [source_pkgname]
        if source_pkgname != binary_name:
            conflicts.append(binary_name)
        provides = [source_pkgname]
        source_url = (
            f"{repo_url}/releases/download/v{version}/"
            f"{binary_name}-v{version}-{LINUX_X86_64_ASSET}.tar.gz"
        )
        source_dir = f"{binary_name}-{version}-{LINUX_X86_64_ASSET}"
        pkgdesc = f"{pkgdesc} (prebuilt binary release)"

    conflicts_block = f"conflicts={shell_array(conflicts)}\n"
    provides_block = ""
    if provides:
        provides_block = f"provides={shell_array(provides)}\n"

    template = Template(args.template.read_text())
    rendered = template.substitute(
        pkgname=args.pkgname,
        pkgver=version,
        pkgdesc=pkgdesc,
        url=package["homepage"],
        license_list=license_list(package["license"]),
        conflicts_block=conflicts_block,
        provides_block=provides_block,
        source_url=source_url,
        source_dir=source_dir,
        binary_name=binary_name,
    )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(rendered)
    print(f"rendered: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
