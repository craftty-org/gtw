# Contributing

Thanks for trying `gtw`. PRs are only accessible via opening an issue first and
then referring to it in the PR.

This project uses [Grimoire tasks](Grimoire.toml) (via `grim`) for common
development workflows. It is optional, but keeps task commands consistent.

## Build

Requirements:

- Rust **1.90+** (2024 edition)
- `pkg-config`, a C toolchain, and CMake (for vendored/bundled native crates)
- ALSA headers on Linux: `libasound2-dev` (Debian/Ubuntu), `alsa-lib` (Arch),
  `alsa-lib-dev` (Alpine). macOS uses CoreAudio.
- On Termux: `pkg install rust clang pkg-config cmake make`

```bash
# build (debug)
grim cast build

# build (release)
grim cast release

# run diagnostics
grim cast doctor
```

Equivalent cargo commands:

```bash
cargo build
cargo build --release
cargo run -- --doctor
```

## Sanity checks

All of these must pass before a PR is merged:

```bash
# run all quality gates (fmt check, clippy, tests, deny)
grim cast ci
```

Or run individual tasks:

```bash
grim cast fmt-check
grim cast clippy
grim cast test
grim cast deny
```

> [!NOTE]
> Also:
>
> - **No live network in tests.** Use fixtures or the in-module mock data.
> - **Every async surface needs states.** Use `Loadable<T>` and
>   `ActivityCenter`; do not add ad-hoc `loading: bool` fields.
> - **Keep the UI thread non-blocking.** Parsing, layout, fetch, and decoding
>   run off the render loop.
> - **Responsive snapshots.** Add or update a loading-state snapshot in
>   `tests/snapshots/` for every new screen or overlay.
> - **Large articles.** The 500 KB layout/parse responsiveness test must remain
>   under its budget.
>
> Regenerate snapshots only when a UI change is intentional:

```bash
grim cast snapshot
```

## Testing media

`grim cast doctor` (or `cargo run -- --doctor`) reports the detected terminal
graphics protocol, multipane/multiplexer state, audio device, and cache health
without touching the network.

Decode tests use generated fixtures, so they work on headless CI. Pixel output
and sound require a real terminal/audio device; include the `--doctor` output
when reporting a preview bug.

## Style

- Keep modules aligned with the existing architecture: `api`, `app`, `cache`,
  `core`, `media`, `render`.
- Prefer small, typed messages and commands over shared mutable state.
- Errors should render as a user-visible state with retry/cancel/fallback, not
  just a log line.
- Use conventional commits, one behaviour per commit:

```text
feat(cache): refresh the sliding TTL on article open
fix(render): wrap wide tables inside the viewport
test(app): snapshot the offline picker
docs(readme): document Wikimedia attribution
```

Do not commit `target/`, cache databases, media downloads, tokens, `.env` files,
or editor swap files. If your PR includes any of those it is automatically
closed.

> [!WARNING]
>
> Version, man page, and completions are generated from the CLI; do not hand-
> edit them. Release packaging scripts live in `scripts/build/`, with platform
> manifests in `dist/`. The release workflow builds the same matrix as
> `craftty-org/gtm` (Debian 12, Arch, musl/Alpine, RPM, macOS arm64, Termux) and
> publishes SHA-256 checksums alongside every asset.
