// crates/mira-app/src/ansi.rs
// ANSI escape code parsing and HTML conversion

use crate::syntax::html_escape;

/// ANSI color codes to CSS colors
fn ansi_to_css_color(code: u8) -> &'static str {
    match code {
        30 => "#1a1a1a", // Black
        31 => "#e06c75", // Red
        32 => "#98c379", // Green
        33 => "#e5c07b", // Yellow
        34 => "#61afef", // Blue
        35 => "#c678dd", // Magenta
        36 => "#56b6c2", // Cyan
        37 => "#abb2bf", // White
        90 => "#5c6370", // Bright Black (Gray)
        91 => "#e06c75", // Bright Red
        92 => "#98c379", // Bright Green
        93 => "#e5c07b", // Bright Yellow
        94 => "#61afef", // Bright Blue
        95 => "#c678dd", // Bright Magenta
        96 => "#56b6c2", // Bright Cyan
        97 => "#ffffff", // Bright White
        _ => "#abb2bf",  // Default
    }
}

/// Convert 256-color code to CSS hex color
fn ansi_256_to_css(n: u8) -> String {
    if n < 16 {
        // Standard colors - use our theme colors
        ansi_to_css_color(if n < 8 { 30 + n } else { 90 + n - 8 }).to_string()
    } else if n < 232 {
        // 216-color cube (6x6x6)
        let n = n - 16;
        let r = (n / 36) % 6;
        let g = (n / 6) % 6;
        let b = n % 6;
        let to_hex = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
        format!("#{:02x}{:02x}{:02x}", to_hex(r), to_hex(g), to_hex(b))
    } else {
        // Grayscale (24 shades)
        let gray = 8 + (n - 232) * 10;
        format!("#{:02x}{:02x}{:02x}", gray, gray, gray)
    }
}

/// Parse style from ANSI escape code parameters
#[derive(Default, Clone)]
pub struct AnsiStyle {
    fg: Option<String>,
    bg: Option<String>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
}

impl AnsiStyle {
    fn to_css(&self) -> String {
        let mut styles = Vec::new();
        if let Some(ref fg) = self.fg {
            styles.push(format!("color:{}", fg));
        }
        if let Some(ref bg) = self.bg {
            styles.push(format!("background:{}", bg));
        }
        if self.bold {
            styles.push("font-weight:bold".to_string());
        }
        if self.dim {
            styles.push("opacity:0.7".to_string());
        }
        if self.italic {
            styles.push("font-style:italic".to_string());
        }
        if self.underline {
            styles.push("text-decoration:underline".to_string());
        }
        styles.join(";")
    }

    fn is_default(&self) -> bool {
        self.fg.is_none() && self.bg.is_none() && !self.bold && !self.dim && !self.italic && !self.underline
    }
}

/// Parse ANSI escape sequence parameters and update style
fn parse_ansi_params(params: &str, style: &mut AnsiStyle) {
    let codes: Vec<u8> = params.split(';').filter_map(|s| s.parse().ok()).collect();
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => *style = AnsiStyle::default(), // Reset
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            22 => { style.bold = false; style.dim = false; }
            23 => style.italic = false,
            24 => style.underline = false,
            30..=37 => style.fg = Some(ansi_to_css_color(codes[i]).to_string()),
            38 => {
                // Extended foreground color
                if i + 2 < codes.len() && codes[i + 1] == 5 {
                    style.fg = Some(ansi_256_to_css(codes[i + 2]));
                    i += 2;
                }
            }
            39 => style.fg = None, // Default foreground
            40..=47 => style.bg = Some(ansi_to_css_color(codes[i] - 10).to_string()),
            48 => {
                // Extended background color
                if i + 2 < codes.len() && codes[i + 1] == 5 {
                    style.bg = Some(ansi_256_to_css(codes[i + 2]));
                    i += 2;
                }
            }
            49 => style.bg = None, // Default background
            90..=97 => style.fg = Some(ansi_to_css_color(codes[i]).to_string()),
            100..=107 => style.bg = Some(ansi_to_css_color(codes[i] - 10).to_string()),
            _ => {}
        }
        i += 1;
    }
}

/// Convert text with ANSI escape codes to HTML with inline styles
pub fn ansi_to_html(text: &str) -> String {
    let mut result = String::new();
    let mut style = AnsiStyle::default();
    let mut chars = text.chars().peekable();
    let mut current_span = String::new();
    let mut in_styled_span = false;

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Start of escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                let mut params = String::new();

                // Read until we hit a letter (the command)
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_alphabetic() {
                        let cmd = chars.next().unwrap();
                        if cmd == 'm' {
                            // Color/style command
                            // Flush current text with current style
                            if !current_span.is_empty() {
                                if in_styled_span {
                                    result.push_str(&html_escape(&current_span));
                                    result.push_str("</span>");
                                    in_styled_span = false;
                                } else {
                                    result.push_str(&html_escape(&current_span));
                                }
                                current_span.clear();
                            }
                            // Update style
                            parse_ansi_params(&params, &mut style);
                        }
                        break;
                    } else {
                        params.push(chars.next().unwrap());
                    }
                }
                continue;
            }
        }

        // Regular character - add to current span
        if current_span.is_empty() && !style.is_default() {
            result.push_str(&format!("<span style=\"{}\">", style.to_css()));
            in_styled_span = true;
        } else if current_span.is_empty() && style.is_default() && in_styled_span {
            in_styled_span = false;
        }
        current_span.push(c);
    }

    // Flush remaining text
    if !current_span.is_empty() {
        result.push_str(&html_escape(&current_span));
        if in_styled_span {
            result.push_str("</span>");
        }
    }

    result
}
