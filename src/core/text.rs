//! Small text-normalisation helpers shared by the API, layout and UI layers.

use scraper::Html;

/// Turn a Wikimedia file title into a readable caption: drop the `File:`
/// namespace, strip the trailing extension, and replace underscores.
pub fn clean_file_title(raw: &str) -> String {
    let raw = strip_html(raw);
    let without_ns = raw
        .strip_prefix("File:")
        .or_else(|| raw.strip_prefix("file:"))
        .unwrap_or(&raw);
    let without_ext = without_ns
        .rsplit_once('.')
        .filter(|(base, ext)| !base.is_empty() && ext.len() <= 5)
        .map(|(base, _)| base)
        .unwrap_or(without_ns);
    without_ext.replace('_', " ").trim().to_string()
}

/// Strip leading `File:` namespace lines that leak into a file-page summary,
/// e.g. `"File:X.jpg\nThe caption"` → `"The caption"`.
pub fn strip_file_prefix(text: &str) -> String {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed
        .strip_prefix("File:")
        .or_else(|| trimmed.strip_prefix("file:"))
    else {
        return trimmed.to_string();
    };
    match rest.find(['\n', '\r']) {
        Some(index) => rest[index + 1..].trim_start().to_string(),
        // A lone `File:Name.ext` with no body carries no readable content.
        None => String::new(),
    }
}

/// Collapse an HTML fragment to whitespace-normalised plain text.
pub fn strip_html(html: &str) -> String {
    let fragment = Html::parse_fragment(html);
    fragment
        .root_element()
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_file_titles() {
        assert_eq!(clean_file_title("File:Example image.jpg"), "Example image");
        assert_eq!(
            clean_file_title("File:Red_panda_(eating).JPG"),
            "Red panda (eating)"
        );
        assert_eq!(clean_file_title("Picture of the day"), "Picture of the day");
    }

    #[test]
    fn strips_file_prefix_lines() {
        assert_eq!(strip_file_prefix("File:X.jpg\nA caption"), "A caption");
        assert_eq!(strip_file_prefix("File:X.jpg"), "");
        assert_eq!(strip_file_prefix("Ordinary summary"), "Ordinary summary");
    }
}
