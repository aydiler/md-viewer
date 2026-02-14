/// Simple HTML table parser for rendering `<table>` blocks as egui grids.
/// Handles `<thead>`, `<tbody>`, `<tr>`, `<th>`, `<td>` elements.
/// No external dependencies — string-based parsing only.

/// Parsed HTML table ready for rendering.
pub struct HtmlTable {
    /// Header rows (from `<thead>` or rows containing `<th>` cells).
    pub header: Vec<Vec<String>>,
    /// Body rows (from `<tbody>` or rows containing `<td>` cells).
    pub rows: Vec<Vec<String>>,
}

/// Parse an HTML block string into a table structure.
/// Returns `None` if no `<table>` is found.
pub fn parse_html_table(html: &str) -> Option<HtmlTable> {
    let lower = html.to_ascii_lowercase();

    let table_start = lower.find("<table")?;
    let table_end = lower.find("</table>")?;
    if table_end <= table_start {
        return None;
    }

    // Find the end of the opening <table...> tag
    let content_start = html[table_start..].find('>')? + table_start + 1;
    let table_content = &html[content_start..table_end];

    let mut header = Vec::new();
    let mut rows = Vec::new();

    // Check if there's a <thead> section
    let lower_content = table_content.to_ascii_lowercase();
    if let Some(thead_start) = lower_content.find("<thead") {
        let thead_content_start = table_content[thead_start..].find('>')? + thead_start + 1;
        let thead_end = lower_content.find("</thead>")?;
        let thead_html = &table_content[thead_content_start..thead_end];
        header = parse_rows(thead_html);
    }

    // Parse <tbody> if present, otherwise parse rows from the whole table content
    let body_html = if let Some(tbody_start) = lower_content.find("<tbody") {
        let tbody_content_start = table_content[tbody_start..].find('>')? + tbody_start + 1;
        let tbody_end = lower_content
            .find("</tbody>")
            .unwrap_or(table_content.len());
        &table_content[tbody_content_start..tbody_end]
    } else {
        // No <tbody> — parse all rows outside <thead>
        if let Some(thead_end_pos) = lower_content.find("</thead>") {
            let after_thead = thead_end_pos + "</thead>".len();
            &table_content[after_thead..]
        } else {
            table_content
        }
    };

    let body_rows = parse_rows(body_html);

    // Separate header rows from body rows when no <thead> was present
    if header.is_empty() {
        for row in body_rows {
            // A row is a header row if it was parsed from <th> cells
            // We detect this by checking if the original HTML for these cells used <th>
            // Since parse_rows already handles this, we need a different approach:
            // Re-check: if no <thead>, any row with <th> cells goes to header
            header.push(row);
        }
        // If all rows ended up as "header" because we couldn't distinguish,
        // check the raw HTML for <th> vs <td>
        if !header.is_empty() {
            let has_th = lower_content.contains("<th");
            let has_td = lower_content.contains("<td");
            if has_th && has_td {
                // Re-parse: split rows by <th> vs <td>
                let all_rows = std::mem::take(&mut header);
                header.clear();
                let tr_positions = find_tag_positions(body_html, "tr");
                for (i, row_cells) in all_rows.into_iter().enumerate() {
                    if i < tr_positions.len() {
                        let tr_html = &tr_positions[i];
                        if tr_html.to_ascii_lowercase().contains("<th") {
                            header.push(row_cells);
                        } else {
                            rows.push(row_cells);
                        }
                    } else {
                        rows.push(row_cells);
                    }
                }
            } else {
                // All same type — first row is header, rest are body
                rows = header.split_off(1.min(header.len()));
            }
        }
    } else {
        rows = body_rows;
    }

    if header.is_empty() && rows.is_empty() {
        return None;
    }

    Some(HtmlTable { header, rows })
}

/// Parse `<tr>` rows from an HTML fragment, extracting cell text from `<td>` and `<th>`.
fn parse_rows(html: &str) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    let lower = html.to_ascii_lowercase();
    let bytes = lower.as_bytes();

    let mut pos = 0;
    while pos < bytes.len() {
        // Find next <tr
        if let Some(tr_offset) = lower[pos..].find("<tr") {
            let tr_start = pos + tr_offset;
            // Find end of opening <tr...> tag
            let Some(tag_end) = lower[tr_start..].find('>') else {
                break;
            };
            let content_start = tr_start + tag_end + 1;

            // Find </tr>
            let tr_end = lower[content_start..]
                .find("</tr>")
                .map(|p| content_start + p)
                .unwrap_or(html.len());

            let tr_content = &html[content_start..tr_end];
            let cells = parse_cells(tr_content);
            if !cells.is_empty() {
                result.push(cells);
            }

            pos = tr_end + "</tr>".len();
        } else {
            break;
        }
    }

    result
}

/// Extract cell text from `<td>` and `<th>` elements within a `<tr>`.
fn parse_cells(html: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut pos = 0;

    while pos < lower.len() {
        // Find next <td or <th
        let td_pos = lower[pos..].find("<td").map(|p| pos + p);
        let th_pos = lower[pos..].find("<th").map(|p| pos + p);

        let cell_start = match (td_pos, th_pos) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };

        // Determine if it's <td or <th (for finding closing tag)
        let is_th = lower[cell_start..].starts_with("<th");
        let close_tag = if is_th { "</th>" } else { "</td>" };

        // Find end of opening tag
        let Some(tag_end) = lower[cell_start..].find('>') else {
            break;
        };
        let content_start = cell_start + tag_end + 1;

        // Find closing tag
        let content_end = lower[content_start..]
            .find(close_tag)
            .map(|p| content_start + p)
            .unwrap_or(html.len());

        let raw = &html[content_start..content_end];
        let text = strip_html_tags(raw);
        let text = decode_entities(&text);
        cells.push(text.trim().to_string());

        pos = content_end + close_tag.len();
    }

    cells
}

/// Collect the raw HTML content of each `<tr>` block for inspection.
fn find_tag_positions(html: &str, _tag: &str) -> Vec<String> {
    let mut result = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut pos = 0;

    while pos < lower.len() {
        if let Some(offset) = lower[pos..].find("<tr") {
            let start = pos + offset;
            let end = lower[start..]
                .find("</tr>")
                .map(|p| start + p + "</tr>".len())
                .unwrap_or(html.len());
            result.push(html[start..end].to_string());
            pos = end;
        } else {
            break;
        }
    }

    result
}

/// Remove HTML tags from a string, preserving text content.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    result
}

/// Decode common HTML entities.
fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&nbsp;", "\u{00A0}")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_table() {
        let html = r#"
        <table>
            <thead>
                <tr><th>Name</th><th>Value</th></tr>
            </thead>
            <tbody>
                <tr><td>Alice</td><td>100</td></tr>
                <tr><td>Bob</td><td>200</td></tr>
            </tbody>
        </table>"#;

        let table = parse_html_table(html).unwrap();
        assert_eq!(table.header.len(), 1);
        assert_eq!(table.header[0], vec!["Name", "Value"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0], vec!["Alice", "100"]);
        assert_eq!(table.rows[1], vec!["Bob", "200"]);
    }

    #[test]
    fn table_without_thead() {
        let html = r#"
        <table>
            <tr><th>Col A</th><th>Col B</th></tr>
            <tr><td>1</td><td>2</td></tr>
            <tr><td>3</td><td>4</td></tr>
        </table>"#;

        let table = parse_html_table(html).unwrap();
        assert_eq!(table.header.len(), 1);
        assert_eq!(table.header[0], vec!["Col A", "Col B"]);
        assert_eq!(table.rows.len(), 2);
    }

    #[test]
    fn table_all_td_no_th() {
        let html = r#"
        <table>
            <tr><td>A</td><td>B</td></tr>
            <tr><td>C</td><td>D</td></tr>
        </table>"#;

        let table = parse_html_table(html).unwrap();
        // First row becomes header when all cells are <td>
        assert_eq!(table.header.len(), 1);
        assert_eq!(table.header[0], vec!["A", "B"]);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0], vec!["C", "D"]);
    }

    #[test]
    fn strips_nested_html() {
        let html = r##"
        <table>
            <tr><td><strong>Bold</strong> text</td><td><a href="#">Link</a></td></tr>
        </table>"##;

        let table = parse_html_table(html).unwrap();
        assert_eq!(table.header[0][0], "Bold text");
        assert_eq!(table.header[0][1], "Link");
    }

    #[test]
    fn decodes_entities() {
        let html = r#"
        <table>
            <tr><td>A &amp; B</td><td>&lt;code&gt;</td></tr>
        </table>"#;

        let table = parse_html_table(html).unwrap();
        assert_eq!(table.header[0][0], "A & B");
        assert_eq!(table.header[0][1], "<code>");
    }

    #[test]
    fn no_table_returns_none() {
        assert!(parse_html_table("<div>Hello</div>").is_none());
        assert!(parse_html_table("plain text").is_none());
    }

    #[test]
    fn tags_with_attributes() {
        let html = r#"
        <table class="data" border="1">
            <tr class="header"><th scope="col">X</th></tr>
            <tr><td style="color:red">Y</td></tr>
        </table>"#;

        let table = parse_html_table(html).unwrap();
        assert_eq!(table.header[0], vec!["X"]);
        assert_eq!(table.rows[0], vec!["Y"]);
    }
}
