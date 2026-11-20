# [gtw](https://github.com/craftty-org/gtw)

<p align="center">
  Wikipedia in your terminal: cached reading, rich rendering, inline media, and offline lists.
  <br />
  <a href="#features">Features</a>
  ·
  <a href="#download">Download</a>
  ·
  <a href="#documentation">Documentation</a>
  ·
  <a href="CONTRIBUTING.md">Contributing</a>
  ·
  <a href="#acknowledgements">Acknowledgements</a>
</p>

`gtw` is a terminal Wikipedia reader written in Rust. It focuses on fast, cached article reading with a TUI, configurable rendering/media behavior, and offline-friendly local data.

## Features

- **Read Wikipedia in a TUI**: keyboard-driven article browsing, search, daily feed, and tabbed reading with session restore.
- **Offline-friendly library**: local cache, `--offline` mode, bookmarks, history, and reading lists.
- **Rich content & media**: HTML-to-terminal rendering, images/math, audio playback, and WebM/VP8/VP9 previews (pure-Rust, no FFmpeg).
- **Customizable & diagnosable**: themes/effects, TOML config, and `--doctor` environment/cache/media diagnostics.

## Download

### Build from source

Requirements:
- Rust 1.90+ (edition 2024)
- A C toolchain and platform build prerequisites for native dependencies (see [CONTRIBUTING.md](CONTRIBUTING.md))

```bash
# debug build
cargo build

# release build
cargo build --release

# run
cargo run -- --doctor
```

### Install locally

```bash
# install binary, man page, and shell completions
make install
```

### Install script

An installer script is included in the repository (`install.sh`). It can install bundled release assets when run from a release archive, or download and install a matching release from GitHub:

```bash
# from the repository root or a release archive
./install.sh --help
./install.sh --prefix "$HOME/.local"
```

Prebuilt release artifacts are produced by the repository's release workflow for supported platforms.

### Install via AUR

No official AUR package is currently provided for `gtw`.

## Documentation

- **CLI help**: `gtw --help`
- **Diagnostics**: `gtw --doctor` (prints config/cache paths, media/terminal capabilities, and library/cache status without contacting the network)
- **Configuration**: default config path is typically `~/.config/gtw/config.toml` (platform-dependent); override with `--config <path>`
- **Cache/logs**: default cache directory is typically `~/.cache/gtw/` with logs at `~/.cache/gtw/gtw.log` unless `--log-file` is set
- **Project docs**: [README.md](README.md), [CONTRIBUTING.md](CONTRIBUTING.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), [Grimoire.toml](Grimoire.toml)

### Manpages

- `gtw(1)` — generated from the CLI (`cargo run -- --man` or `make man`, output in `artifacts/man/gtw.1`)

Shell completions can be generated with `gtw --completions <shell>` or via `make completions`.

### Dependencies

`gtw` is built with Rust and uses a set of crates for TUI, networking, caching, parsing, and media/rendering. Key direct dependencies include:

- **TUI/runtime**: `ratatui`, `crossterm`, `tachyonfx`, `tuikit`, `tokio`
- **Networking & data**: `reqwest`, `serde`/`serde_json`, `toml`, `rusqlite`
- **Parsing/rendering**: `scraper`, `syntect`, `unicode-width`, `image`, `resvg`, `ratatui-image`, `ratex-*`
- **Media**: `symphonia`, `rodio`, `zvidlib`

For the complete and authoritative dependency list (including versions and features), see [Cargo.toml](Cargo.toml) and [Cargo.lock](Cargo.lock).

## Contributing

Contributions, bug reports, and focused patches are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before contributing.

Key project expectations include:
- Rust 1.90+ (edition 2024)
- Passing quality gates: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace`, `cargo deny check`
- No live network in tests
- No FFmpeg/runtime FFmpeg dependency
- Keep the UI thread non-blocking and use explicit loading/activity states

## Screenshots

Screenshots are not yet included in this repository.

## Acknowledgements

- [Wikipedia](https://www.wikipedia.org/) and the [Wikimedia](https://www.wikimedia.org/) projects for the knowledge and content this client is designed to read.
- The Rust and open-source ecosystem, including libraries used by this project such as [ratatui](https://ratatui.rs/), [crossterm](https://github.com/crossterm-rs/crossterm), [tokio](https://tokio.rs/), [reqwest](https://github.com/seanmonstar/reqwest), [rusqlite](https://github.com/rusqlite/rusqlite), [symphonia](https://github.com/pdeljanov/Symphonia), [rodio](https://github.com/RustAudio/rodio), [resvg](https://github.com/linebender/resvg), and [syntect](https://github.com/trishume/syntect).

## Contributors

<!-- CONTRIBUTORS -->
<p align="left">
  <a href="https://github.com/prjctimg"><img src="https://github.com/prjctimg.png?size=80" width="50" height="50" style="border-radius:50%;margin:4px;" alt="prjctimg"/></a> <a href="https://github.com/vaishnav-sabari-girish"><img src="https://github.com/vaishnav-sabari-girish.png?size=80" width="50" height="50" style="border-radius:50%;margin:4px;" alt="vaishnav-sabari-girish"/></a>
</p>
<!-- /CONTRIBUTORS -->

(c) 2026, [prjctimg](https://prjctimg.me)

Released under the GPL-3.0 license. See [LICENSE](LICENSE) for details.
