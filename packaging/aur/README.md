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

## Publish to the AUR

```bash
git clone ssh://aur@aur.archlinux.org/prman.git
cd prman
cp /path/to/prm/packaging/aur/out/source/PKGBUILD .
cp /path/to/prm/packaging/aur/out/source/.SRCINFO .
git add PKGBUILD .SRCINFO
git commit -m 'Initial import'
git push

git clone ssh://aur@aur.archlinux.org/prman-bin.git
cd prman-bin
cp /path/to/prm/packaging/aur/out/bin/PKGBUILD .
cp /path/to/prm/packaging/aur/out/bin/.SRCINFO .
git add PKGBUILD .SRCINFO
git commit -m 'Initial import'
git push
```

## Notes

- `prman` builds the stable tagged source release from GitHub.
- `prman-bin` repackages the published Linux x86_64 release tarball from GitHub Releases.
- `updpkgsums` must be run after rendering so the placeholder checksum is replaced.
- The current default AUR package name is `prman`.
- The current default binary package name is `prman-bin`.
- Because the installed executable is `prm`, both rendered packages conflict with a package literally named `prm`.
- `prman` and `prman-bin` also conflict with each other because they install the same executable.
