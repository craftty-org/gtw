use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

struct LogoLine {
    text: Line<'static>,
}

/// Colours lifted from the Wikipedia mark: blue ocean, warm landmass, and a
/// cool rim around the puzzle pieces.
const GLOBE_BLUE: Color = Color::Rgb(41, 90, 140);
const GLOBE_DEEP: Color = Color::Rgb(28, 63, 104);
const GLOBE_LAND: Color = Color::Rgb(226, 226, 214);
const GLOBE_EDGE: Color = Color::Rgb(120, 150, 180);

/// 16x16 puzzle-globe bitmap. `#` ocean, `%` deep ocean, `O` landmass/piece,
/// `.` empty. Two bitmap rows share one terminal cell via a half block.
#[rustfmt::skip]
const GLOBE: [&str; 16] = [
    "     %%%%%%     ",
    "   %%%%%%%%%%   ",
    "  %%%######%%%  ",
    " %%%##OOOO##%%% ",
    " %%%#OOOOOO#%%% ",
    "%%%##OO##OO##%%%",
    "%%%#OO####OO#%%%",
    "%%##OO##OO##OO##",
    "%%##OO#OO##OO#%%",
    "%%%#OOO####OO%%%",
    " %%%#OOOOOOOO#%%",
    " %%%##OOOOOO#%%%",
    "  %%###OOOO#%%% ",
    "   %%%%%%%##%%  ",
    "     %%%%%%     ",
    "       %%       ",
];

fn logo_lines(accent: Color) -> Vec<LogoLine> {
    let globe: Vec<Vec<char>> = GLOBE
        .iter()
        .map(|row| {
            row.chars()
                .map(|c| if c == ' ' { '.' } else { c })
                .collect()
        })
        .collect();
    let word: [&str; 8] = ["W", "I", "K", "I", "P", "E", "D", "I"];

    let mut lines = Vec::new();
    for pair in 0..globe.len() / 2 {
        let top = &globe[pair * 2];
        let bottom = &globe[pair * 2 + 1];
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(top.len() + 1);
        for col in 0..top.len() {
            let t = top[col];
            let b = bottom[col];
            let (glyph, fg) = match (t, b) {
                ('.', '.') => (' ', GLOBE_EDGE),
                ('.', b) => ('▄', color_of(b)),
                (t, '.') => ('▀', color_of(t)),
                (t, b) if t == b => ('█', color_of(t)),
                (t, _) => ('▀', color_of(t)),
            };
            spans.push(Span::styled(glyph.to_string(), Style::default().fg(fg)));
        }
        // Sit the wordmark beside the globe so the lockup reads as a logo.
        spans.push(Span::raw("  "));
        if let Some(letter) = word.get(pair) {
            spans.push(Span::styled(
                letter.to_string(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(LogoLine {
            text: Line::from(spans),
        });
    }

    // Trailing "A" row keeps the silhouette tapering to a point at the bottom.
    lines.push(LogoLine {
        text: Line::from(vec![
            Span::raw(" ".repeat(GLOBE[15].len())),
            Span::raw("  "),
            Span::styled(
                "A",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
        ]),
    });

    lines
}

fn color_of(pixel: char) -> Color {
    match pixel {
        '#' => GLOBE_BLUE,
        '%' => GLOBE_DEEP,
        'O' => GLOBE_LAND,
        _ => GLOBE_EDGE,
    }
}

#[allow(dead_code)]
pub fn logo_preview() -> Vec<String> {
    logo_lines(GLOBE_EDGE)
        .into_iter()
        .map(|line| {
            line.text
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_has_expected_shape() {
        let rows = logo_preview();
        assert_eq!(rows.len(), 9);
        assert!(rows.iter().all(|row| !row.trim().is_empty()));
    }
}
