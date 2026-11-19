# In8

*Pronounced like "innate" — your PC, ready from birth.*

> Work in progress — currently in the planning phase.

A cross-platform desktop app (Windows + Linux) that gets a freshly formatted PC ready to use in one go:
install your everyday apps, remove bloatware, apply system tweaks and run post-install configuration.
Inspired by [Ninite](https://ninite.com), with profiles, reversible tweaks and an editable catalog.

## Features (planned)

- **Apps** — pick from a curated catalog and install everything in one run
  (winget on Windows; Flatpak first with apt/dnf/pacman fallback on Linux, so it works on any distro).
- **Profiles** — one click selects a whole set of apps and tweaks (e.g. Development, Gaming).
- **Debloat** — remove preinstalled Windows apps you don't use.
- **Tweaks** — dark mode, file extensions, Explorer and desktop settings, all reversible.
- **Post-install** — configure Git, create folders, clone dotfiles, install VS Code extensions.
- **Dry run** — preview every command before anything touches your system.
- **Export / import** — save your selection and reuse it on another machine.

## Tech stack

[Tauri v2](https://tauri.app) · Rust · Svelte 5 · TypeScript · Tailwind CSS

## Development

Requirements: Rust, Node.js LTS and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev     # run in development
npm run tauri build   # build for the current OS (.exe on Windows, AppImage/.deb on Linux)
```

## License

TBD
