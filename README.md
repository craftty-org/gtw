# gtw

**Wikipedia in your terminal.** Read articles, follow links, browse the daily
featured feed, search offline, and preview images, audio, video, GIFs, code, and
LaTeX math — without leaving the TUI.

`gtw` is a mouse-first [Ratatui](https://ratatui.rs) application written in Rust.
It keeps a compressed offline library, stays responsive while fetching and
laying out large articles, and degrades cleanly when the network, a terminal
graphics protocol, or an audio device is unavailable.

> **Status:** `v0.1.0` is a read-only reader. Search, the daily feed, offline
> cache, bookmarks, history, reading lists, media previews, math, themes,
> mouse navigation, in-page search, link peeking, the Alt+e article outline,
> tabs and jump-to-date pickers, and a command palette are implemented.
> Account login, editing, and watchlists are planned for a later release.

## Features

- **Responsive article rendering** — headings, paragraphs, lists, tables,
  blockquotes, references, links, infobox text, and wrapped code blocks that
  reflow on resize instead of clipping.
- **Mouse-first navigation** — click the feed tabs, story cards, article links,
  images, list rows, pickers, media controls, and overlays. The scroll wheel
  works everywhere. Every mouse action also has a keyboard equivalent.
- **Daily Wikipedia feed** — featured article, picture of the day, in the news,
  did you know, on this day, and most read, with date navigation, a jump-to-date
  calendar (`alt+d`), deduplicated stories, and a bottom preview pane.
- **Pickers for everything** — search Wikipedia and the local cache, browse
  offline articles, create and open reading lists, add the current article to a
  list, jump between screens, walk the current article's related links, headings,
  images, code and audio, and switch themes.
- **Compressed sliding cache** — article and feed JSON is compressed with zstd
  inside SQLite and decompressed on read. The default TTL is three days and is
  refreshed every time an article is opened. Bookmarks and reading-list entries
  are pinned and exempt from expiry.
- **Rich media** — PNG/JPEG/GIF/WebP/BMP/TGA/ICO/TIFF/SVG images and animated
  GIFs, OGG/Opus/Vorbis/MP3/FLAC/WAV audio, WebM VP8/VP9 video previews, and
  syntax-highlighted code.
- **LaTeX math** — inline and display math rendered through RaTeX, rasterized
  with resvg, and placed in the document with an ASCII fallback when a terminal
  cannot display images.
- **16 themes** — light and dark palettes, instantly switchable, with a
  pixel-art Wikipedia puzzle-globe splash drawn in half blocks and TachyonFX
  transitions.
- **Explicit states** — every async surface has loading, stale, empty, offline,
  error, and retry states through `Loadable<T>` and the activity center.
- **Diagnostics** — `gtw --doctor` reports terminal graphics support, audio,
  cache health, and media capabilities without touching the network.

## Install

### Release installer

The installer detects Debian/Ubuntu, Arch, musl/Alpine, macOS, and Termux,
downloads the matching archive, verifies its SHA-256 checksum, and installs the
binary, man page, completions, and license under `~/.local` by default.

```bash
curl -fsSL https://raw.githubusercontent.com/prjctimg/gtw/main/install.sh | bash
```

Pin a release or choose a prefix:

```bash
./install.sh --version 0.1.0
./install.sh --version 0.1.0 --prefix /usr/local
```

When run from inside a downloaded release archive, the same script installs the
bundled files instead of downloading anything.

### Packages

Every release publishes:

| Platform | Assets |
|---|---|
| Debian 12+ | `gtw-debian-12-x86_64.tar.gz`, `gtw-debian-12-aarch64.tar.gz`, `.deb` |
| Arch Linux | `gtw-arch-x86_64.tar.gz`, `gtw-arch-aarch64.tar.gz`, `.pkg.tar.zst` |
| musl / Alpine | `gtw-x86_64-linux-musl.tar.gz`, `gtw-aarch64-linux-musl.tar.gz`, `.apk` |
| Fedora/RHEL | `.rpm` for x86_64 and aarch64 |
| macOS | `gtw-aarch64-darwin.tar.gz` (Apple Silicon) |
| Termux | `gtw-aarch64-android.tar.gz`, `gtw-android-0.1.0-aarch64.deb` |

Install a package directly if you prefer:

```bash
sudo apt install ./gtw_0.1.0_amd64.deb
sudo dnf install ./gtw-0.1.0.x86_64.rpm
sudo pacman -U ./gtw-0.1.0-x86_64.pkg.tar.zst
sudo apk add --allow-untrusted ./gtw-0.1.0-r0-x86_64.apk
```

### From source

Requirements: Rust 1.90+ (edition 2024), `pkg-config`, and ALSA development
headers on Linux (`libasound2-dev` on Debian/Ubuntu, `alsa-lib` on Arch,
`alsa-lib-dev` on Alpine). macOS uses CoreAudio and needs no extra audio package.

```bash
git clone https://github.com/prjctimg/gtw
cd gtw
cargo build --release
./target/release/gtw
```

## Use

```bash
gtw                 # start on the daily feed
gtw --offline       # read the cache without contacting Wikipedia
gtw --theme Chadrula
gtw --demo-media    # open the Media Lab and load the selected preview
gtw --doctor        # environment report, no network
gtw --no-animations # disable TachyonFX transitions
```

Configuration lives in `~/.config/gtw/config.toml`; the cache database, media
cache, and logs live in `~/.cache/gtw/`. Set `--config` or `--log-file` to
override those paths.

### Keyboard and mouse

| Action | Mouse | Keys |
|---|---|---|
| Search Wikipedia/offline cache | click the search field | `/`, `ctrl+p` |
| Command palette | — | `:`, `;` |
| Navigation picker | — | `alt+space` |
| Article outline picker | — | `alt+e` (`[` / `]` cycle filters, `p` play audio) |
| Find in article | — | `ctrl+f` (↑/↓/enter jump, esc close) |
| Jump-to-date calendar | — | `alt+d` |
| Open tabs picker | — | `alt+\`` (`x` closes a tab) |
| Offline library picker | — | `o`, `ctrl+o`, `alt+o` |
| Home / saved / history | — | `h`, `b`, `y`, `alt+s`, `alt+y` |
| Reading lists | — | `l`, `ctrl+l`, `alt+l` |
| Themes (live preview) | — | `p`, `alt+t` (↑/↓ preview · enter apply · esc revert) |
| Activity center | — | `A`, `alt+a` |
| New reading list | click **new list** in the picker | `n`, `ctrl+n` |
| Bookmarks | — | `B` |
| Add to a list | — | `a` |
| Move / scroll | wheel, click rows and links | `j`/`k`, arrows, `g`/`G`, `pgup`/`pgdn` |
| Peek at a link | hover a link | `ctrl+k` |
| Table of contents | click **toc** in the outline picker | `alt+c` |
| Open article or link | click | `enter` |
| Browser | — | `o` while reading an article |
| Refresh | — | `r`, `R` |
| Help / activity | — | `?`, `A` |
| Quit / close | click the × or outside an overlay | `q`, `ctrl+c`, `esc` |

Media Lab (`m`) contains one item for every preview path: animated raster
image, SVG, highlighted code, LaTeX math, audio, and WebM video. `--demo-media`
opens it directly, which is the quickest way to verify a terminal after install.

## Media support

| Type | Formats | Path |
|---|---|---|
| Images | PNG, JPEG, GIF, WebP, BMP, TGA, ICO, TIFF, SVG | `image` + `ratatui-image` |
| Animation | Animated GIF (frame timing) | `image`, terminal image protocol |
| Code | 100+ syntaxes and themes | `syntect` |
| Math | KaTeX/AMS-style LaTeX subset | RaTeX → SVG → `resvg` |
| Audio | OGG/Opus, Vorbis, MP3, FLAC, WAV | Symphonia + rodio |
| Video | WebM VP8/VP9 previews (180 frames, 640 px wide) | pure-Rust `zvidlib` |

Images use the best protocol the terminal reports: Kitty graphics, iTerm2,
Sixel, or a half-block fallback. Run `gtw --doctor` to see the detected protocol
and whether tmux/Screen passthrough is active. In tmux, allow passthrough and
use a graphics-capable terminal for pixel previews.

## Cache and offline behaviour

- Articles and feeds are stored as zstd-compressed JSON blobs in SQLite, with an
  FTS5 index for offline search.
- `CACHE_TTL_DAYS` defaults to **3**. Opening an article refreshes
  `last_opened_at`, so the expiry window slides forward on every read.
- Bookmarks and articles referenced by a reading list are pinned and survive
  both TTL purges and size-budget eviction.
- The default cache budget is 512 MB. `ctrl+r` runs a purge; the Settings screen
  can toggle offline mode.
- A cache hit is labelled in the UI, and `--offline` never opens a network
  connection.

## Data sources and attribution

`gtw` is an independent client, not affiliated with or endorsed by the Wikimedia
Foundation.

- Article HTML, search results, and page metadata: the MediaWiki Action API at
  `<language>.wikipedia.org/w/api.php` (parse/search).
- Daily feed sections: the Wikimedia REST feed at
  `api.wikimedia.org/feed/v1/wikipedia/<language>/featured/YYYY/MM/DD`, with the
  On This Day endpoint as a fallback.
- Images and media: files and thumbnails served by Wikimedia Commons and the
  Wikipedia upload pipeline.
- Wikipedia text is available under
  [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/); individual
  Commons files may use other licenses. gtw links each article back to its
  source page; do not strip attribution when sharing content.
- Requests send a descriptive `User-Agent` and are rate-limited and retried with
  bounded backoff. There is no bulk crawling or editing in v0.1.0.

## Dependencies

The important ones, and why they are here:

| Crate | Role |
|---|---|
| `ratatui`, `crossterm` | TUI, layout, keyboard/mouse input |
| `tokio`, `futures`, `tokio-util` | async runtime, cancellation, loaders |
| `reqwest` (rustls) | Wikipedia/Wikimedia HTTP with no OpenSSL requirement |
| `scraper` | Wikipedia HTML extraction |
| `rusqlite` (bundled SQLite), `zstd` | compressed cache, FTS5 search |
| `serde`, `serde_json`, `toml`, `chrono` | data, config, feed dates |
| `ratatui-image`, `image`, `resvg` | terminal images, decoding, SVG/math raster |
| `ratex-parser`, `ratex-layout`, `ratex-svg` | KaTeX-style math |
| `syntect` | code highlighting |
| `symphonia`, `rodio` | audio decode/playback |
| `zvidlib` | pure-Rust VP8/VP9 WebM preview decode |
| `tachyonfx` | screen and feed transitions |
| `clap` | CLI, man page, shell completions |

There is intentionally **no FFmpeg** binary, crate, or runtime dependency. The
release workflow and CI enforce that, and video is decoded in process by
`zvidlib`.

## Inspiration and credits

- **[nullspace](https://github.com/samox73/nullspace)** — a Ratatui LaTeX
  library with beautiful inline math. gtw follows its approach of using RaTeX's
  KaTeX-compatible pipeline, rasterizing formulas for the terminal, caching the
  result, and keeping a readable text fallback. The implementation here is
  independent, but the inspiration is direct and gratefully acknowledged.
- **[prjctimg/gtm](https://github.com/prjctimg/gtm)** — gtw's 16 theme palettes,
  wordmark sizing logic, release matrix, and installer conventions are adapted
  from gtm, also GPL-3.0.
- Wikipedia, Wikimedia Commons, and the Wikimedia REST/Action APIs provide the
  content and data this client displays.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
cargo run -- --doctor
```

Tests never use live Wikimedia requests. Loading-state snapshots live in
`tests/snapshots/`; regenerate intentional UI changes with:

```bash
UPDATE_SNAPSHOTS=1 cargo test snapshot_tests
```

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the media/parse test harness and
commit conventions, and [`specs/`](specs/README.md) for the original design.

## License

GPL-3.0-only. See [`LICENSE`](LICENSE).

The theme palettes and pixel-art mark are adapted from `prjctimg/gtm`
(GPL-3.0). Wikipedia content is CC BY-SA 4.0 and remains the property of its
contributors.
