# prm Release Guide

`prm` releases are tag-driven, but the tag is only valid if it already points at a commit whose tracked release files match the version.

That means the supported flow is:

1. prepare the release locally
2. verify the generated packaging files locally
3. push the release commit
4. push the annotated `vX.Y.Z` tag
5. let GitHub Actions publish the release, Homebrew formula, and AUR packages

The release workflow no longer edits `main` or retags commits for you.

## One-Time Prerequisites

The release pipeline assumes all of the following already exist:

- this repository has GitHub Actions enabled for releases
- GitHub repository secret `HOMEBREW_TAP_TOKEN` exists and has `contents:write` access to `bencetotht/homebrew-prm`
- GitHub repository secret `AUR_SSH_PRIVATE_KEY` exists and can push to:
  - `ssh://aur@aur.archlinux.org/prman.git`
  - `ssh://aur@aur.archlinux.org/prman-bin.git`
- the Homebrew tap repository `bencetotht/homebrew-prm` exists and has a default branch
- the AUR packages `prman` and `prman-bin` already exist and are writable by the SSH key

If you created differently named secrets earlier, rename them or update the workflow before releasing. The checked-in workflow expects `HOMEBREW_TAP_TOKEN` and `AUR_SSH_PRIVATE_KEY` exactly.

## What The Workflows Do

### CI

`.github/workflows/ci.yml` runs on pushes to `main` and on pull requests:

- installs the Rust toolchain from `rust-toolchain.toml`
- checks formatting
- runs clippy with warnings denied
- runs the Rust test suite

### Nix flake check

`.github/workflows/nix-flake-check.yml` runs on Linux only:

- installs Nix
- runs `nix flake check --system x86_64-linux -L`

This is useful validation, but it is not a Homebrew or AUR publish gate.

### Release

`.github/workflows/release.yml` runs on:

- pushes of tags matching `v*.*.*`
- manual workflow dispatch

For every release run it:

1. normalizes the release version from the tag or workflow input
2. verifies that `Cargo.toml`, `Cargo.lock`, and `README.md` are already synced to that version
3. builds release archives for:
   - `x86_64-unknown-linux-gnu`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
4. assembles checksums and release metadata
5. renders `dist/prm.rb` and validates it with `ruby -c`

When publishing is enabled it then:

6. creates the GitHub Release and uploads the tarballs plus checksum file
7. updates `bencetotht/homebrew-prm` with `Formula/prm.rb`
8. renders and validates AUR `PKGBUILD` files for `prman` and `prman-bin`
9. builds and validates the AUR packages
10. pushes `PKGBUILD` and `.SRCINFO` to the matching AUR repositories

The workflow is intentionally all-or-nothing for a real release. A GitHub Release is not considered complete until the Homebrew and AUR publish jobs also finish successfully.

## Local Preflight

Run these before every release:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
python3 scripts/sync_version.py --check vX.Y.Z
```

If you want a local packaging smoke test too:

```bash
python3 scripts/render_homebrew_formula.py \
  --version X.Y.Z \
  --github-owner bencetotht \
  --github-repo prm \
  --template .github/homebrew/prm.rb.template \
  --output /tmp/prm.rb \
  --linux-x86-64-sha256 deadbeef \
  --darwin-x86-64-sha256 deadbeef \
  --darwin-arm64-sha256 deadbeef
ruby -c /tmp/prm.rb

python3 scripts/render_aur_pkgbuild.py \
  --pkgname prman \
  --variant source \
  --template packaging/aur/PKGBUILD.in \
  --output /tmp/prman.PKGBUILD
bash -n /tmp/prman.PKGBUILD

python3 scripts/render_aur_pkgbuild.py \
  --pkgname prman-bin \
  --variant bin \
  --template packaging/aur/PKGBUILD-bin.in \
  --output /tmp/prman-bin.PKGBUILD
bash -n /tmp/prman-bin.PKGBUILD
```

## Supported Dry Runs

### Local release-prep dry run

Verify that the repository is already ready for a release without editing anything:

```bash
python3 scripts/prepare_release.py vX.Y.Z --check-only
```

This fails if:

- the worktree is dirty
- the tag already exists
- tracked release files are not synced to `vX.Y.Z`

### Remote workflow dry run

After pushing a branch that already contains the synced release commit, rehearse the workflow without publishing:

```bash
gh workflow run release.yml --ref <branch-with-release-commit> -f version=vX.Y.Z -f publish=false
```

That dry run still verifies the versioned files and builds the release artifacts, but it does not:

- publish a GitHub Release
- update the Homebrew tap
- push to AUR

## Creating A Real Release

Use the helper to prepare the commit and tag locally:

```bash
python3 scripts/prepare_release.py vX.Y.Z
```

That command:

- normalizes the version
- updates `Cargo.toml`, `Cargo.lock`, and `README.md`
- verifies the update with `scripts/sync_version.py --check`
- creates a local commit named `release: vX.Y.Z`
- creates an annotated tag named `vX.Y.Z`

Then push the commit and tag:

```bash
git push origin HEAD
git push origin vX.Y.Z
```

The tag push is what triggers the real publish flow.

## Post-Release Verification

After the workflow finishes, verify all three distribution surfaces.

### GitHub Release

Confirm the `vX.Y.Z` release exists in `bencetotht/prm` with:

- three tarballs
- one combined checksum file
- release notes

### Homebrew

Confirm `bencetotht/homebrew-prm` contains an updated `Formula/prm.rb` that points to the same `vX.Y.Z` GitHub Release assets.

Install smoke test:

```bash
brew install bencetotht/prm/prm
prm --version
```

If Homebrew already had an older release installed:

```bash
brew update
brew upgrade prm
prm --version
```

### AUR

Confirm both AUR repos were updated:

- `prman`
- `prman-bin`

For an install smoke test on an Arch machine:

```bash
yay -S prman-bin
prm --version
```

If you want to validate the source package instead:

```bash
yay -S prman
prm --version
```

## Failure Recovery

### Version file verification failed

The tag points at the wrong commit. Fix it locally instead of trying to patch the release in CI:

1. update the branch with `python3 scripts/prepare_release.py vX.Y.Z`
2. push the new commit
3. delete and recreate the tag only if you intentionally want to replace an unpublished bad tag

### GitHub Release published but Homebrew or AUR failed

Treat the release as incomplete.

Use the failed workflow logs to determine whether the problem was:

- invalid secret permissions
- tap repo branch protection
- missing AUR package access
- packaging validation failure

Once the underlying issue is fixed, re-run the failed jobs from GitHub Actions if possible. If not, create a new release tag only after cleaning up the partial external state.

### Homebrew manual fallback

If the tap update fails after `dist/prm.rb` was rendered successfully:

1. download the `release-metadata` workflow artifact
2. copy `prm.rb` to `Formula/prm.rb` in `bencetotht/homebrew-prm`
3. commit and push the tap update manually

### AUR manual fallback

If AUR publish fails after the AUR artifacts were built:

1. download `aur-prman` and `aur-prman-bin`
2. copy `PKGBUILD` and `.SRCINFO` into each corresponding AUR git repo
3. commit and push each repo manually

## Packaging Targets

- Homebrew tap: `bencetotht/homebrew-prm`
- AUR source package: `prman`
- AUR binary package: `prman-bin`
- Executable installed by all packages: `prm`
