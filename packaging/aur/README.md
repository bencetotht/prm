# AUR packaging

This repository now supports two AUR packages:

- `prman`: source build package
- `prman-bin`: prebuilt binary release package

Both install the executable as `prm`.

## Render locally

```bash
python3 scripts/render_aur_pkgbuild.py \
  --pkgname prman \
  --variant source \
  --template packaging/aur/PKGBUILD.in \
  --output packaging/aur/out/source/PKGBUILD

python3 scripts/render_aur_pkgbuild.py \
  --pkgname prman-bin \
  --variant bin \
  --template packaging/aur/PKGBUILD-bin.in \
  --output packaging/aur/out/bin/PKGBUILD
```

## Release pipeline

The release workflow now:

- builds and validates the AUR packages after publishing a GitHub release
- uploads two workflow artifacts:
  - `aur-prman`
  - `aur-prman-bin`
- pushes `PKGBUILD` and `.SRCINFO` to the AUR automatically when the
  `AUR_SSH_PRIVATE_KEY` GitHub Actions secret is configured

Each artifact contains:

- `PKGBUILD`
- `.SRCINFO`
- the built package archive produced by `makepkg`

## Finish the packages locally

```bash
cd packaging/aur/out/source
updpkgsums
makepkg --printsrcinfo > .SRCINFO
namcap PKGBUILD .SRCINFO
makepkg --syncdeps --cleanbuild --clean

cd ../bin
updpkgsums
makepkg --printsrcinfo > .SRCINFO
namcap PKGBUILD .SRCINFO
makepkg --syncdeps --cleanbuild --clean
```

## Manual fallback

If the AUR publish job is disabled or the `AUR_SSH_PRIVATE_KEY` secret is unavailable, download the
matching artifact and push its `PKGBUILD` and `.SRCINFO` to the AUR git repository manually.

```bash
git clone ssh://aur@aur.archlinux.org/prman.git
cd prman
cp /path/to/downloaded/aur-prman/PKGBUILD .
cp /path/to/downloaded/aur-prman/.SRCINFO .
git add PKGBUILD .SRCINFO
git commit -m 'Initial import'
git push

git clone ssh://aur@aur.archlinux.org/prman-bin.git
cd prman-bin
cp /path/to/downloaded/aur-prman-bin/PKGBUILD .
cp /path/to/downloaded/aur-prman-bin/.SRCINFO .
git add PKGBUILD .SRCINFO
git commit -m 'Initial import'
git push
```

## Notes

- `prman` builds the stable tagged source release from GitHub.
- `prman-bin` repackages the published Linux x86_64 release tarball from GitHub Releases.
- The standalone AUR workflow was removed; AUR build and publish now happen in the release pipeline.
- `updpkgsums` must be run after rendering so the placeholder checksum is replaced.
- The current default AUR package name is `prman`.
- The current default binary package name is `prman-bin`.
- Because the installed executable is `prm`, both rendered packages conflict with a package literally named `prm`.
- `prman` and `prman-bin` also conflict with each other because they install the same executable.
