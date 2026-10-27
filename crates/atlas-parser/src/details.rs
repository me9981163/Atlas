use tree_sitter::Node;

/// Collapse runs of whitespace so a multi-line declaration fits on one line
/// in the database and in search results.
pub(crate) fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// Extract the declaration header: everything from the node start to the
/// body (when present), with the trailing `{`, `;` or `:` removed.
pub(crate) fn declaration_signature(
    node: Node,
    source: &str,
    body: Option<Node>,
    strip_trailing_colon: bool,
) -> Option<String> {
    let start = node.start_byte();
    let end = body
        .map(|body| body.start_byte())
        .unwrap_or_else(|| node.end_byte())
        .min(node.end_byte());
    if end <= start {
        return None;
    }
    let mut text = collapse_whitespace(&source[start..end]);
    loop {
        let trimmed = text.trim_end();
        if let Some(stripped) = trimmed.strip_suffix('{') {
            text = stripped.trim_end().to_string();
        } else if let Some(stripped) = trimmed.strip_suffix(';') {
            text = stripped.trim_end().to_string();
        } else if strip_trailing_colon {
            if let Some(stripped) = trimmed.strip_suffix(':') {
                text = stripped.trim_end().to_string();
            } else {
                break;
            }
        } else {
            break;
        }
    }
    if text.is_empty() {
        None
    } else {
        Some(truncate_chars(&text, 500))
    }
}

/// Collect contiguous documentation comments immediately before a node.
/// Attributes/decorators between the comments and the declaration are
/// skipped; any other node ends the block.
pub(crate) fn collect_preceding_docs<F, G>(
    node: Node,
    source: &str,
    is_doc: F,
    clean: G,
) -> Option<String>
where
    F: Fn(&str) -> bool,
    G: Fn(&str) -> Option<String>,
{
    let mut parts: Vec<String> = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sibling) = current {
        match sibling.kind() {
            "attribute_item" | "decorator" => {}
            "line_comment" | "block_comment" | "comment" => {
                let text = &source[sibling.byte_range()];
                if !is_doc(text) {
                    break;
                }
                if let Some(cleaned) = clean(text) {
                    parts.push(cleaned);
                }
            }
            _ => break,
        }
        current = sibling.prev_sibling();
    }
    if parts.is_empty() {
        None
    } else {
        parts.reverse();
        let joined = parts.join("\n");
        let joined = joined.trim();
        if joined.is_empty() {
            None
        } else {
            Some(truncate_chars(joined, 4000))
        }
    }
}

pub(crate) fn clean_line_doc(text: &str) -> Option<String> {
    let text = text.trim();
    let text = text
        .strip_prefix("///")
        .or_else(|| text.strip_prefix("//!"))
        .or_else(|| text.strip_prefix("//"))
        .or_else(|| text.strip_prefix('#'))
        .unwrap_or(text);
    let text = text.strip_prefix(' ').unwrap_or(text).trim_end();
    Some(text.to_string())
}

pub(crate) fn clean_block_doc(text: &str) -> Option<String> {
    let text = text.trim();
    let inner = text
        .strip_prefix("/**")
        .or_else(|| text.strip_prefix("/*!"))
        .or_else(|| text.strip_prefix("/*"))
        .unwrap_or(text);
    let inner = inner.strip_suffix("*/").unwrap_or(inner);
    let mut lines: Vec<String> = inner
        .lines()
        .map(|line| {
            let line = line.trim();
            let line = line.strip_prefix('*').unwrap_or(line);
            line.strip_prefix(' ')
                .unwrap_or(line)
                .trim_end()
                .to_string()
        })
        .collect();
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

pub(crate) fn clean_python_string(text: &str) -> Option<String> {
    let text = text.trim();
    let mut index = 0;
    for ch in text.chars() {
        if matches!(ch, 'r' | 'R' | 'b' | 'B' | 'u' | 'U' | 'f' | 'F') {
            index += ch.len_utf8();
        } else {
            break;
        }
    }
    let text = &text[index..];
    let (quote, quote_len) = if text.starts_with("\"\"\"") {
        ("\"\"\"", 3)
    } else if text.starts_with("'''") {
        ("'''", 3)
    } else if text.starts_with('"') {
        ("\"", 1)
    } else if text.starts_with('\'') {
        ("'", 1)
    } else {
        return None;
    };
    if text.len() < quote_len * 2 || !text.ends_with(quote) {
        return None;
    }
    let inner = &text[quote_len..text.len() - quote_len];
    let mut lines: Vec<String> = inner.lines().map(str::to_string).collect();
    // Mirror Python's inspect.cleandoc: the indentation of the lines after
    // the first one is removed, and a first-line summary keeps its position.
    let mut margin = usize::MAX;
    for line in lines.iter().skip(1) {
        let content = line.trim_start();
        if !content.is_empty() {
            margin = margin.min(line.len() - content.len());
        }
    }
    if let Some(first) = lines.first_mut() {
        *first = first.trim_start().to_string();
    }
    if margin != usize::MAX {
        for line in lines.iter_mut().skip(1) {
            *line = line
                .get(margin..)
                .unwrap_or_else(|| line.trim_start())
                .to_string();
        }
    }
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return None;
    }
    let joined = lines.join("\n");
    let joined = joined.trim();
    if joined.is_empty() {
        None
    } else {
        Some(truncate_chars(joined, 4000))
    }
}
