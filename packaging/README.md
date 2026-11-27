# Packaging

Everything here is driven by `.github/workflows/release.yml`. When `main`
carries a new version, the workflow builds the Admin Center (Tauri) and the
agent, server and CLI binaries, then repackages them into every format below and
attaches them to the GitHub Release. To rebuild only the packages for an
existing release, run the **Release** workflow by hand (Actions > Release > Run
workflow) and enter the tag, for example `v0.12.0`, under *repackage*.

## What each release carries

| Asset | Built by | Notes |
| --- | --- | --- |
| `OpenDirectory-AdminCenter-windows-x64.msi`, `-windows-x64-setup.exe` | Tauri, renamed by `linux-packages` | Stable names, so `releases/latest/download/<name>` always works |
| `OpenDirectory-AdminCenter-macos-universal.dmg` | Tauri | |
| `OpenDirectory-AdminCenter-linux-amd64.deb`, `-x86_64.rpm`, `-x86_64.AppImage` | Tauri | The `.rpm` also serves openSUSE |
| `OpenDirectory-AdminCenter-linux-x86_64.pkg.tar.zst` | `linux/build-admin-center-arch.sh` | Arch package repackaged from the `.deb` with nfpm |
| `OpenDirectory-AdminCenter-linux-amd64.snap` | `snap/snapcraft.yaml` | |
| `OpenDirectory-AdminCenter-linux-x86_64.flatpak` | `flatpak/` | Bundle for `flatpak install --user` |
| `OpenDirectory-Agent-windows-x64.msi` | `windows/agent.wxs` (WiX v5) | `SERVER`, `TOKEN`, `FINGERPRINT` properties enroll unattended |
| `OpenDirectory-Agent-macos-universal.pkg` | `macos/build-agent-pkg.sh` | Reads a staged `enroll.conf` |
| `opendirectory-{agent,server,cli}-linux-*.deb/.rpm/.pkg.tar.zst` | `linux/build-packages.sh` (nfpm) | The agent package reads `/etc/opendirectory/enroll.conf` |
| `install.sh`, `install.ps1` | `install/` | One-line Admin Center installers |
| `SHA256SUMS` | `publish` job | Checked by both install scripts |
| `store-manifests.tar.gz` | `render-manifests.sh` | Flathub manifest, AUR `PKGBUILD`, Homebrew cask and Nix pin, filled in for this release |

No Alpine `.apk`: the binaries link against glibc and Alpine uses musl. Alpine
users can run the Admin Center from the Flatpak.

## One-time setup (Dan)

These steps need your own accounts or repository settings, so the workflow
can't do them. Each one is optional: until it's done, the matching job skips
itself and the rest of the release still publishes.

1. **Let `gh` push workflow files.** The `gh` token on the dev machine lacks the
   `workflow` scope, so pushes that touch `.github/workflows` are rejected:

   ```bash
   gh auth refresh -h github.com -s workflow
   ```

2. **APT and DNF repositories (GitHub Pages).**
   - Settings > Pages > Build and deployment > Source: **GitHub Actions**.
   - Settings > Environments > `github-pages` > Deployment branches: add `main`
     (the default branch isn't `main`, so Pages would refuse the deploy).
   - Create the signing key and store it as the `REPO_SIGNING_KEY` secret:

     ```bash
     packaging/repo/create-signing-key.sh ~/opendirectory-keys
     ```

     Keep the backup it writes somewhere safe and out of the repository.

3. **Homebrew tap.** Create the public repository `Lin8x/homebrew-opendirectory`
   (empty is fine), then a fine-grained token with *Contents: read and write* on
   that repository only, and save it as the `HOMEBREW_TAP_TOKEN` secret:

   ```bash
   gh repo create Lin8x/homebrew-opendirectory --public --description "Homebrew tap for OpenDirectory"
   gh secret set HOMEBREW_TAP_TOKEN --repo Lin8x/OpenDirectory
   ```

4. **Snap Store.** Sign in at https://snapcraft.io with an Ubuntu One account,
   register the name, then export credentials for CI:

   ```bash
   sudo snap install snapcraft --classic
   snapcraft login
   snapcraft register opendirectory-admin-center
   snapcraft export-login --snaps=opendirectory-admin-center --channels=stable --acls=package_access,package_push,package_update,package_release - | gh secret set SNAPCRAFT_STORE_CREDENTIALS --repo Lin8x/OpenDirectory
   ```

   The snap uses strict confinement, so it doesn't need a manual store review.

5. **Flathub.** Flathub builds from its own repository, one per app:
   - Choose a license for OpenDirectory and add a `LICENSE` file. Flathub
     requires one: add `<project_license>` (an SPDX id such as `Apache-2.0`)
     to `flatpak/io.github.lin8x.OpenDirectory.metainfo.xml`.
   - Fork https://github.com/flathub/flathub, branch from `new-pr`, and add the
     rendered `flatpak/io.github.lin8x.OpenDirectory.yml` and `.metainfo.xml`
     from the release's `store-manifests.tar.gz`, plus `shared-modules` as a git
     submodule (`git submodule add https://github.com/flathub/shared-modules.git`).
   - Open the pull request against `new-pr` and follow the reviewers. After it's
     merged you get `flathub/io.github.lin8x.OpenDirectory`; for each release,
     update the version and `sha256` there (Flathub's bot can do this through
     `x-checker-data`).
   - The app ID `io.github.lin8x.OpenDirectory` is verified through your GitHub
     account, so no domain is needed.

6. **AUR.** Create an account at https://aur.archlinux.org and add your SSH
   public key to it, then:

   ```bash
   git clone ssh://aur@aur.archlinux.org/opendirectory-admin-center-bin.git
   # copy PKGBUILD and .SRCINFO from store-manifests.tar.gz (aur/opendirectory-admin-center-bin/)
   cd opendirectory-admin-center-bin && git add PKGBUILD .SRCINFO && git commit -m "Update to <version>" && git push
   ```

   The `PKGBUILD` says `license=('custom')` until the repository has a license.

7. **Nix.** The flake in `nix/` reads `nix/sources.json`. After a release,
   replace that file with the one in `store-manifests.tar.gz` (`nix/sources.json`)
   in the next PR.

8. **Code signing (later).** Without signatures, Windows SmartScreen warns,
   macOS needs right-click > Open, and Intune refuses the agent `.pkg`. Signing
   needs a Windows code-signing certificate and an Apple Developer ID
   ($99/year).

## Building locally

```bash
packaging/linux/build-packages.sh v0.12.0 <dir-with-linux-binaries> out/   # needs nfpm
packaging/render-manifests.sh v0.12.0 <dir-with-release-assets> rendered/
OD_RELEASE_BASE=http://127.0.0.1:8000 sh packaging/install/install.sh --dry-run
```
