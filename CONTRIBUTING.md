# Contributing

Thanks for trying `gtw`. It is a small hobby project; bug reports and focused
patches are welcome. PRs are limited while the project settles before `v1`, but
you can always open an issue with a reproduction.

## Build

Requirements:

- Rust **1.90+** with edition 2024 support
- `pkg-config`, a C toolchain, and CMake (for vendored/bundled native crates)
- ALSA headers on Linux: `libasound2-dev` (Debian/Ubuntu), `alsa-lib` (Arch),
  `alsa-lib-dev` (Alpine). macOS uses CoreAudio.
- On Termux: `pkg install rust clang pkg-config cmake make`

```bash
cargo build --release
./target/release/gtw --doctor
```

The release archive installer can be tested locally without publishing:

```bash
./scripts/build/manpages.sh artifacts
GTW_BIN=target/release/gtw ./scripts/build/completions.sh "$(pwd)/artifacts"
```

## Quality gates

All of these must pass before a PR is merged:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
```

Also:

- **No live network in tests.** Use fixtures or the in-module mock data.
- **No FFmpeg.** No binary, crate, or runtime dependency; video stays with the
  pure-Rust `zvidlib` pipeline.
- **Every async surface needs states.** Use `Loadable<T>` and `ActivityCenter`;
  do not add ad-hoc `loading: bool` fields.
- **Keep the UI thread non-blocking.** Parsing, layout, fetch, and decoding run
  off the render loop.
- **Responsive snapshots.** Add or update a loading-state snapshot in
  `tests/snapshots/` for every new screen or overlay.
- **Large articles.** The 500 KB layout/parse responsiveness test must remain
  under its budget.

Regenerate snapshots only when a UI change is intentional:

```bash
UPDATE_SNAPSHOTS=1 cargo test snapshot_tests
```

## Testing media

`gtw --doctor` reports the detected terminal graphics protocol, multipane/multiplexer state, audio device, and cache health without touching the network.

Decode tests use generated fixtures, so they work on headless CI. Pixel output and sound require a real terminal/audio device; include the `--doctor` output when reporting a preview bug.

## Style

- Rust edition 2024; `rustfmt` defaults.
- Keep modules aligned with the existing architecture: `api`, `app`, `cache`,
  `core`, `media`, `render`.
- Prefer small, typed messages and commands over shared mutable state.
- Errors should render as a user-visible state with retry/cancel/fallback, not
  just a log line.
- Conventional Commits, one behaviour per commit:

```text
feat(cache): refresh the sliding TTL on article open
fix(render): wrap wide tables inside the viewport
test(app): snapshot the offline picker
docs(readme): document Wikimedia attribution
```

Do not commit `target/`, cache databases, media downloads, tokens, `.env` files,
or editor swap files.

## Packaging

Version, man page, and completions are generated from the CLI; do not hand-edit
them. Release packaging scripts live in `scripts/build/`, with platform
manifests in `dist/`. The release workflow builds the same matrix as
`craftty-org/gtm` (Debian 12, Arch, musl/Alpine, RPM, macOS arm64, Termux) and
publishes SHA-256 checksums alongside every asset.
