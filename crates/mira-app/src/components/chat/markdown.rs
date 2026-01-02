// crates/mira-app/src/components/chat/markdown.rs
// Simple markdown renderer

use leptos::prelude::*;
use super::CodeBlock;

#[component]
pub fn Markdown(content: String) -> impl IntoView {
    let blocks = parse_markdown(&content);

    view! {
        <div class="markdown">
            {blocks.into_iter().map(|block| {
                match block {
                    MarkdownBlock::Paragraph(text) => {
                        view! { <p inner_html=render_inline(&text)></p> }.into_any()
                    }
                    MarkdownBlock::CodeBlock { language, code } => {
                        view! { <CodeBlock code=code language=language.unwrap_or_else(|| "text".to_string())/> }.into_any()
                    }
                    MarkdownBlock::UnorderedList(items) => {
                        view! {
                            <ul>
                                {items.into_iter().map(|item| {
                                    view! { <li inner_html=render_inline(&item)></li> }
                                }).collect::<Vec<_>>()}
                            </ul>
                        }.into_any()
                    }
                    MarkdownBlock::OrderedList(items) => {
                        view! {
                            <ol>
                                {items.into_iter().map(|item| {
                                    view! { <li inner_html=render_inline(&item)></li> }
                                }).collect::<Vec<_>>()}
                            </ol>
                        }.into_any()
                    }
                    MarkdownBlock::Blockquote(text) => {
                        view! { <blockquote inner_html=render_inline(&text)></blockquote> }.into_any()
                    }
                    MarkdownBlock::HorizontalRule => {
                        view! { <hr/> }.into_any()
                    }
                }
            }).collect::<Vec<_>>()}
        </div>
    }
}

enum MarkdownBlock {
    Paragraph(String),
    CodeBlock { language: Option<String>, code: String },
    UnorderedList(Vec<String>),
    OrderedList(Vec<String>),
    Blockquote(String),
    HorizontalRule,
}

fn parse_markdown(content: &str) -> Vec<MarkdownBlock> {
    let mut blocks = Vec::new();
    let mut lines = content.lines().peekable();
    let mut current_paragraph = String::new();

    while let Some(line) = lines.next() {
        // Code block
        if line.starts_with("```") {
            // Flush paragraph
            if !current_paragraph.is_empty() {
                blocks.push(MarkdownBlock::Paragraph(current_paragraph.trim().to_string()));
                current_paragraph = String::new();
            }

            let language = line.strip_prefix("```").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let mut code = String::new();
            while let Some(code_line) = lines.next() {
                if code_line.starts_with("```") {
                    break;
                }
                if !code.is_empty() {
                    code.push('\n');
                }
                code.push_str(code_line);
            }
            blocks.push(MarkdownBlock::CodeBlock { language, code });
            continue;
        }

        // Horizontal rule
        if line.trim() == "---" || line.trim() == "***" || line.trim() == "___" {
            if !current_paragraph.is_empty() {
                blocks.push(MarkdownBlock::Paragraph(current_paragraph.trim().to_string()));
                current_paragraph = String::new();
            }
            blocks.push(MarkdownBlock::HorizontalRule);
            continue;
        }

        // Blockquote
        if line.starts_with("> ") {
            if !current_paragraph.is_empty() {
                blocks.push(MarkdownBlock::Paragraph(current_paragraph.trim().to_string()));
                current_paragraph = String::new();
            }
            let mut quote = line.strip_prefix("> ").unwrap_or(line).to_string();
            while let Some(next) = lines.peek() {
                if next.starts_with("> ") {
                    quote.push(' ');
                    quote.push_str(lines.next().unwrap().strip_prefix("> ").unwrap_or(""));
                } else {
                    break;
                }
            }
            blocks.push(MarkdownBlock::Blockquote(quote));
            continue;
        }

        // Unordered list
        if line.starts_with("- ") || line.starts_with("* ") {
            if !current_paragraph.is_empty() {
                blocks.push(MarkdownBlock::Paragraph(current_paragraph.trim().to_string()));
                current_paragraph = String::new();
            }
            let mut items = vec![line[2..].to_string()];
            while let Some(next) = lines.peek() {
                if next.starts_with("- ") || next.starts_with("* ") {
                    items.push(lines.next().unwrap()[2..].to_string());
                } else {
                    break;
                }
            }
            blocks.push(MarkdownBlock::UnorderedList(items));
            continue;
        }

        // Ordered list
        if line.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && line.contains(". ")
        {
            if let Some(pos) = line.find(". ") {
                if line[..pos].chars().all(|c| c.is_ascii_digit()) {
                    if !current_paragraph.is_empty() {
                        blocks.push(MarkdownBlock::Paragraph(current_paragraph.trim().to_string()));
                        current_paragraph = String::new();
                    }
                    let mut items = vec![line[pos + 2..].to_string()];
                    while let Some(next) = lines.peek() {
                        if let Some(np) = next.find(". ") {
                            if next[..np].chars().all(|c| c.is_ascii_digit()) {
                                items.push(lines.next().unwrap()[np + 2..].to_string());
                                continue;
                            }
                        }
                        break;
                    }
                    blocks.push(MarkdownBlock::OrderedList(items));
                    continue;
                }
            }
        }

        // Empty line - flush paragraph
        if line.trim().is_empty() {
            if !current_paragraph.is_empty() {
                blocks.push(MarkdownBlock::Paragraph(current_paragraph.trim().to_string()));
                current_paragraph = String::new();
            }
            continue;
        }

        // Regular text
        if !current_paragraph.is_empty() {
            current_paragraph.push(' ');
        }
        current_paragraph.push_str(line);
    }

    // Flush remaining paragraph
    if !current_paragraph.is_empty() {
        blocks.push(MarkdownBlock::Paragraph(current_paragraph.trim().to_string()));
    }

    blocks
}

fn render_inline(text: &str) -> String {
    let mut result = html_escape(text);

    // Bold: **text** or __text__
    result = regex_replace(&result, r"\*\*(.+?)\*\*", "<strong>$1</strong>");
    result = regex_replace(&result, r"__(.+?)__", "<strong>$1</strong>");

    // Italic: *text* or _text_
    result = regex_replace(&result, r"\*(.+?)\*", "<em>$1</em>");
    result = regex_replace(&result, r"_(.+?)_", "<em>$1</em>");

    // Inline code: `code`
    result = regex_replace(&result, r"`([^`]+)`", "<code>$1</code>");

    // Links: [text](url)
    result = regex_replace(&result, r"\[([^\]]+)\]\(([^)]+)\)", r#"<a href="$2">$1</a>"#);

    result
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn regex_replace(text: &str, pattern: &str, replacement: &str) -> String {
    // Simple regex replacement without external crate
    // For a more robust solution, we'd use the regex crate
    // This is a simplified implementation
    let mut result = text.to_string();

    // Handle **bold**
    if pattern == r"\*\*(.+?)\*\*" {
        while let Some(start) = result.find("**") {
            if let Some(end) = result[start + 2..].find("**") {
                let inner = &result[start + 2..start + 2 + end];
                let replacement_text = format!("<strong>{}</strong>", inner);
                result = format!("{}{}{}", &result[..start], replacement_text, &result[start + 4 + end..]);
            } else {
                break;
            }
        }
        return result;
    }

    // Handle __bold__
    if pattern == r"__(.+?)__" {
        while let Some(start) = result.find("__") {
            if let Some(end) = result[start + 2..].find("__") {
                let inner = &result[start + 2..start + 2 + end];
                let replacement_text = format!("<strong>{}</strong>", inner);
                result = format!("{}{}{}", &result[..start], replacement_text, &result[start + 4 + end..]);
            } else {
                break;
            }
        }
        return result;
    }

    // Handle `code`
    if pattern == r"`([^`]+)`" {
        while let Some(start) = result.find('`') {
            if let Some(end) = result[start + 1..].find('`') {
                let inner = &result[start + 1..start + 1 + end];
                let replacement_text = format!("<code>{}</code>", inner);
                result = format!("{}{}{}", &result[..start], replacement_text, &result[start + 2 + end..]);
            } else {
                break;
            }
        }
        return result;
    }

    // Handle [text](url)
    if pattern == r"\[([^\]]+)\]\(([^)]+)\)" {
        while let Some(start) = result.find('[') {
            if let Some(text_end) = result[start + 1..].find("](") {
                let text = &result[start + 1..start + 1 + text_end];
                let url_start = start + 1 + text_end + 2;
                if let Some(url_end) = result[url_start..].find(')') {
                    let url = &result[url_start..url_start + url_end];
                    let replacement_text = format!(r#"<a href="{}">{}</a>"#, url, text);
                    result = format!("{}{}{}", &result[..start], replacement_text, &result[url_start + url_end + 1..]);
                    continue;
                }
            }
            break;
        }
        return result;
    }

    // Handle *italic* (but not **)
    if pattern == r"\*(.+?)\*" {
        let chars: Vec<char> = result.chars().collect();
        let mut new_result = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '*' && (i + 1 >= chars.len() || chars[i + 1] != '*') {
                // Find closing *
                if let Some(end) = chars[i + 1..].iter().position(|&c| c == '*') {
                    let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                    new_result.push_str(&format!("<em>{}</em>", inner));
                    i = i + 2 + end;
                    continue;
                }
            }
            new_result.push(chars[i]);
            i += 1;
        }
        return new_result;
    }

    // Handle _italic_ (but not __)
    if pattern == r"_(.+?)_" {
        let chars: Vec<char> = result.chars().collect();
        let mut new_result = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '_' && (i + 1 >= chars.len() || chars[i + 1] != '_') {
                // Find closing _
                if let Some(end) = chars[i + 1..].iter().position(|&c| c == '_') {
                    let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                    new_result.push_str(&format!("<em>{}</em>", inner));
                    i = i + 2 + end;
                    continue;
                }
            }
            new_result.push(chars[i]);
            i += 1;
        }
        return new_result;
    }

    result
}
