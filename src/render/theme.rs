#[allow(dead_code)]
use crate::core::{AppTheme, ThemeEntry};
use ratatui::style::Color;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ThemesFile {
    themes: Vec<ThemeDef>,
}

#[derive(Debug, Deserialize)]
struct ThemeDef {
    name: String,
    light: bool,
    bg: String,
    pane_bg: Option<String>,
    picker_bg: String,
    elevated_bg: Option<String>,
    muted_border: Option<String>,
    fg: String,
    fg_dim: String,
    fg_bright: String,
    accent: String,
    secondary_accent: Option<String>,
    tertiary_accent: Option<String>,
    error: String,
    warning: String,
    success: String,
    selection_fg: String,
    selection_bg: String,
    border: String,
    border_active: String,
    volume_low: Option<String>,
    volume_medium: Option<String>,
    volume_high: Option<String>,
    sidebar_active_border: Option<String>,
    notification_border: Option<String>,
    monochromatic: Option<bool>,
}

fn parse_color(s: &str) -> Color {
    let s = s.trim().trim_start_matches('#');
    if s.len() == 6
        && let Ok(r) = u8::from_str_radix(&s[0..2], 16)
        && let Ok(g) = u8::from_str_radix(&s[2..4], 16)
        && let Ok(b) = u8::from_str_radix(&s[4..6], 16)
    {
        return Color::Rgb(r, g, b);
    }
    Color::White
}

fn load_from_str(s: &str) -> Vec<ThemeEntry> {
    let Ok(t) = toml::from_str::<ThemesFile>(s) else {
        return vec![];
    };
    t.themes
        .into_iter()
        .map(|d| {
            let pane_bg = d
                .pane_bg
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.bg));
            let elevated_bg = d
                .elevated_bg
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.picker_bg));
            let muted_border = d
                .muted_border
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.border));
            let notification_border = d
                .notification_border
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.border_active));
            let secondary_accent = d
                .secondary_accent
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.accent));
            let tertiary_accent = d
                .tertiary_accent
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.accent));
            let volume_low = d
                .volume_low
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.success));
            let volume_medium = d
                .volume_medium
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.warning));
            let volume_high = d
                .volume_high
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.error));
            let sidebar_active_border = d
                .sidebar_active_border
                .as_ref()
                .map(|x| parse_color(x))
                .unwrap_or_else(|| parse_color(&d.border_active));
            ThemeEntry {
                name: d.name,
                light: d.light,
                theme: AppTheme {
                    bg: parse_color(&d.bg),
                    pane_bg,
                    picker_bg: parse_color(&d.picker_bg),
                    elevated_bg,
                    muted_border,
                    fg: parse_color(&d.fg),
                    fg_dim: parse_color(&d.fg_dim),
                    fg_bright: parse_color(&d.fg_bright),
                    accent: parse_color(&d.accent),
                    secondary_accent,
                    tertiary_accent,
                    error: parse_color(&d.error),
                    warning: parse_color(&d.warning),
                    success: parse_color(&d.success),
                    selection_fg: parse_color(&d.selection_fg),
                    selection_bg: parse_color(&d.selection_bg),
                    border: parse_color(&d.border),
                    border_active: parse_color(&d.border_active),
                    volume_low,
                    volume_medium,
                    volume_high,
                    sidebar_active_border,
                    notification_border,
                    monochromatic: d.monochromatic.unwrap_or(false),
                },
            }
        })
        .collect()
}

pub fn load_bundled_themes() -> Vec<ThemeEntry> {
    const THEMES_TOML: &str = include_str!("assets/themes.toml");
    load_from_str(THEMES_TOML)
}



#[allow(dead_code)]
pub fn merged_themes() -> Vec<ThemeEntry> {
    load_bundled_themes()
}
