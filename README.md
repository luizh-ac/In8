# In8

*Pronounced like "innate" — your PC, ready from birth.*

A desktop app for Windows and Linux that gets a freshly formatted PC ready in one go: install your everyday apps,
remove bloatware, apply system tweaks and run post-install setup. Inspired by [Ninite](https://ninite.com), with
profiles, reversible tweaks and an editable catalog.

## Features

- **Apps** — pick from a curated catalog and install everything in one run: winget (and verified GitHub releases)
  on Windows; Flatpak first with apt, dnf or pacman on Linux, asking for your password once per run.
- **WSL** — keep languages and dev tools inside Linux on Windows: install WSL and a distro, turn on systemd,
  Docker Engine without Docker Desktop, VS Code connected to WSL, `.wslconfig`.
- **Profiles** — one click selects a whole set (My setup, Essentials, Dev on WSL, Gaming, Media & chat), or save your own.
- **Debloat** — remove preinstalled Windows apps you don't use.
- **Tweaks** — dark mode, Explorer, taskbar, privacy and performance settings; every one can be reverted.
- **Post-install** — Git name and email, SSH key, VS Code extensions, toolchains, projects folder, dotfiles.
- **Dry run** — preview every command before anything touches your system.
- **Backup** — export your selection and saved profiles to a file and import it after formatting.

## Tech stack

[Tauri v2](https://tauri.app) · Rust · Svelte 5 · TypeScript · Tailwind CSS

## Development

Requirements: Rust, Node.js LTS and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev            # run in development
npm run tauri build          # Windows: NSIS installer in src-tauri/target/release/bundle/nsis/
bash scripts/build-linux.sh  # Linux (or WSL): AppImage and .deb in src-tauri/target/linux-bundle/
```

`scripts/sandbox.ps1` opens a release build in Windows Sandbox, to try tweaks and debloat without touching your PC.

## Releasing

1. Bump the version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`, and commit.
2. Create the draft release: `gh release create v0.2.0 --draft --title "In8 v0.2.0" --notes "…"`.
3. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
4. The Release workflow builds the Windows installer, the AppImage and the `.deb`, signs the update files
   and attaches them to the draft. Publish the draft: from then on, In8 offers that version as an update.

Updates are signed with a key kept outside the repository; CI reads it from the `TAURI_SIGNING_PRIVATE_KEY` secret.

## License

[MIT](LICENSE)
