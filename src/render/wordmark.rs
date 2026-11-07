use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

/// Pixel-art lockup for the gtw splash: a half-block Wikipedia puzzle globe
/// beside the WIKIPEDIA wordmark. Every glyph is a plain Unicode block or half
/// block, so the mark keeps its pixel-art feel on every terminal, whether or
/// not it speaks a terminal graphics protocol.
///
/// The globe is a 16x16 bitmap; `▀`, `▄` and `█` pack two pixels into one cell
/// for a crisp silhouette, and each puzzle region keeps its own colour.
#[allow(dead_code)]
pub fn render_wordmark(area: Rect, buf: &mut Buffer, color: Color) {
    let lines = logo_lines(color);
    let height = lines.len() as u16;
    if area.height < height || area.width < 18 {
        return;
    }
    let offset = area.y + area.height.saturating_sub(height) / 2;
    for (row, line) in lines.into_iter().enumerate() {
        let y = offset + row as u16;
        if y >= area.bottom() {
            break;
        }
        if line.width == 0 || area.x + line.width > area.right() {
            continue;
        }
        Widget::render(
            Paragraph::new(line.text),
            Rect::new(area.x, y, line.width, 1),
            buf,
        );
    }
}

struct LogoLine {
    text: Line<'static>,
    width: u16,
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
        let width = spans
            .iter()
            .map(|span| span.content.chars().count() as u16)
            .sum();
        lines.push(LogoLine {
            text: Line::from(spans),
            width,
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
        width: GLOBE[15].len() as u16 + 3,
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

/// A compact globe mark for the header: three rows of half blocks that read as
/// the puzzle globe at a glance, with the row width kept under 12 cells.
pub fn render_globe_mark(area: Rect, buf: &mut Buffer, accent: Color) {
    if area.width < 6 || area.height < 3 {
        return;
    }
    let rows = [" ▄███▄ ", "███O███", " ▀███▀ "];
    for (row, text) in rows.iter().enumerate() {
        let y = area.y + row as u16;
        if y >= area.bottom() {
            break;
        }
        let width = (text.chars().count() as u16).min(area.width);
        Widget::render(
            Paragraph::new(Span::styled(
                text.to_string(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            )),
            Rect::new(area.x, y, width, 1),
            buf,
        );
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
