use std::io::IsTerminal;
use std::path::Path;

use anyhow::Result;

use crate::cache::Cache;
use crate::core::Config;
use crate::media::audio::AudioEngine;
use crate::render::images::ImageRenderer;
use crate::render::theme::merged_themes;

/// Print the information a user (or a bug report) needs to understand why a
/// terminal preview, cache, or audio device is not behaving as expected.
pub fn run(config: &Config, config_path: Option<&Path>) -> Result<()> {
    println!("gtw {}", env!("CARGO_PKG_VERSION"));
    println!(
        "build      {} / {}",
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    println!("config     {}", display_path(config_path));
    println!(
        "cache      {} ({})",
        Config::cache_dir().display(),
        if Config::cache_dir().is_dir() {
            "present"
        } else {
            "not created yet"
        }
    );
    println!("media dir  {}", Config::media_dir().display());
    println!(
        "theme      {} ({} bundled themes)",
        config.theme.name,
        merged_themes().len()
    );
    println!(
        "cache ttl  {} days (sliding; bookmarks and lists pinned)",
        config.cache.ttl_days
    );
    println!();

    match Cache::open() {
        Ok(cache) => match cache.library() {
            Ok(library) => {
                println!("cache db   ok · {}", cache.path().display());
                println!(
                    "library    {} articles · {} feeds · {} bookmarks · {} reading lists · {}",
                    library.articles.len(),
                    library.feeds.len(),
                    library.bookmarks.len(),
                    library.lists.len(),
                    bytes_label(library.bytes)
                );
            }
            Err(error) => println!("cache db   open, but library query failed: {error}"),
        },
        Err(error) => println!("cache db   FAILED: {error}"),
    }

    println!(
        "terminal   TERM={} COLORTERM={} tty(stdin)={} tty(stdout)={}",
        env_or("TERM", "?"),
        env_or("COLORTERM", "?"),
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal()
    );
    let images = ImageRenderer::new();
    println!(
        "graphics   {} (ratatui-image; halfblocks always available)",
        images.protocol_name()
    );
    println!(
        "multiplex  tmux={} screen={} kitty={} wezterm={}",
        std::env::var_os("TMUX").is_some(),
        std::env::var_os("STY").is_some(),
        std::env::var_os("KITTY_WINDOW_ID").is_some(),
        std::env::var_os("WEZTERM_EXECUTABLE").is_some()
    );
    match AudioEngine::new() {
        Ok(engine) => {
            drop(engine);
            println!("audio      output device available (Symphonia + rodio)");
        }
        Err(error) => println!("audio      unavailable: {error}"),
    }
    println!(
        "images     PNG JPEG GIF WebP BMP TGA ICO TIFF SVG{}",
        if config.media.enabled {
            ""
        } else {
            " (disabled in config)"
        }
    );
    println!(
        "video      WebM VP8 / VP9 previews (pure Rust zvidlib; no ffmpeg runtime){}",
        if config.media.enabled {
            ""
        } else {
            " (disabled in config)"
        }
    );
    println!(
        "audio      OGG/Opus Vorbis MP3 FLAC WAV via Symphonia{}",
        if config.media.enabled {
            ""
        } else {
            " (disabled in config)"
        }
    );
    println!(
        "math       KaTeX subset via RaTeX + resvg{}",
        if config.render.syntax_highlight {
            ""
        } else {
            " · syntax highlighting disabled"
        }
    );
    println!(
        "effects    tachyonfx{}",
        if config.effects.enabled && !config.general.no_animations {
            ""
        } else {
            " disabled"
        }
    );
    println!("offline    {}", config.general.offline);
    println!("network    not contacted by --doctor (safe for air-gapped checks)");
    Ok(())
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn display_path(path: Option<&Path>) -> String {
    match path {
        Some(path) => format!(
            "{} ({})",
            path.display(),
            if path.exists() { "present" } else { "default" }
        ),
        None => Config::path().display().to_string(),
    }
}

fn bytes_label(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
