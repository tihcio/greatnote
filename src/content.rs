//! Conversione del documento strutturato (JSON Tiptap v1) verso e da formati
//! aperti. L'importazione costruisce soltanto nodi e contrassegni ammessi dallo
//! schema: l'HTML non viene mai conservato, quindi script, attributi e stili
//! sono scartati per costruzione.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};
use scraper::{ElementRef, Html, Node};
use serde_json::{Map, Value, json};

use crate::{
    domain::note::{FONT_FAMILIES, FONT_SIZES, is_hex_color},
    secret_text::is_armored_ciphertext,
};

/// Profondità massima dei contenitori di blocco (documento incluso): mantiene
/// i documenti importati entro il limite di annidamento della validazione.
const MAX_BLOCK_STACK: usize = 14;
const MAX_HTML_DEPTH: usize = 256;
const MAX_LINK_LENGTH: usize = 2048;
const MARK_ORDER: [&str; 6] = ["link", "bold", "italic", "strike", "underline", "code"];
/// Linguaggio dei blocchi di codice che contengono un testo cifrato: in
/// Markdown e HTML il nodo `encryptedText` diventa un blocco `age`, e un
/// blocco `age` valido importato torna a essere un testo cifrato.
const ENCRYPTED_TEXT_LANGUAGE: &str = "age";
/// Segnaposto del testo cifrato nel testo semplice (ricerca, AI, anteprime).
const ENCRYPTED_TEXT_PLACEHOLDER: &str = "🔒";

/// Testo age del nodo `encryptedText`, se valido.
fn ciphertext(node: &Value) -> Option<&str> {
    attr_str(node, "ciphertext").filter(|text| is_armored_ciphertext(text))
}

/// Copia del documento in cui ogni testo cifrato è un blocco di codice `age`
/// con il testo cifrato: per l'esportazione in testo semplice, che
/// altrimenti mostrerebbe solo il segnaposto.
pub fn encrypted_text_as_code(document: &Value) -> Value {
    match document {
        Value::Object(object)
            if object.get("type").and_then(Value::as_str) == Some("encryptedText") =>
        {
            json!({
                "type": "codeBlock",
                "attrs": { "language": ENCRYPTED_TEXT_LANGUAGE },
                "content": [{ "type": "text", "text": ciphertext(document).unwrap_or_default() }],
            })
        }
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), encrypted_text_as_code(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(encrypted_text_as_code).collect()),
        other => other.clone(),
    }
}

/// Metadati opzionali scritti nel front matter Markdown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontMatter {
    pub title: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub tags: Vec<String>,
    pub notebook: Option<String>,
    pub attachments: Vec<String>,
    /// Esportazione Markdown: id dell'allegato → percorso relativo, per le
    /// immagini nel testo.
    pub image_paths: std::collections::HashMap<String, String>,
}

thread_local! {
    /// Percorsi delle immagini per la conversione Markdown in corso.
    static IMAGE_PATHS: std::cell::RefCell<std::collections::HashMap<String, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Note di testo scritte sull'immagine (annotazioni `text`).
fn image_notes(node: &Value) -> Vec<String> {
    node.pointer("/attrs/annotations")
        .and_then(Value::as_array)
        .map(|shapes| {
            shapes
                .iter()
                .filter(|shape| shape.get("kind").and_then(Value::as_str) == Some("text"))
                .filter_map(|shape| shape.get("text").and_then(Value::as_str))
                .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|text| !text.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn image_alt(node: &Value) -> String {
    attr_str(node, "alt")
        .map(str::trim)
        .filter(|alt| !alt.is_empty())
        .unwrap_or("immagine")
        .to_owned()
}

/// Risultato di un'importazione: documento valido e metadati riconosciuti.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedDocument {
    pub title: Option<String>,
    pub document: Value,
    pub front_matter: FrontMatter,
}

pub fn is_safe_link(href: &str) -> bool {
    let href = href.trim();
    if href.is_empty()
        || href.len() > MAX_LINK_LENGTH
        || href.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return false;
    }
    let lower = href.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

/// Verifica un timestamp UTC nel formato usato dal database
/// (`AAAA-MM-GGTHH:MM:SS[.fff]Z`).
pub fn is_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes.len() > 30 || bytes.last() != Some(&b'Z') {
        return false;
    }
    let pattern = b"dddd-dd-ddTdd:dd:dd";
    let fixed = bytes[..19]
        .iter()
        .zip(pattern)
        .all(|(byte, expected)| match expected {
            b'd' => byte.is_ascii_digit(),
            other => byte == other,
        });
    let fraction = &bytes[19..bytes.len() - 1];
    fixed
        && (fraction.is_empty()
            || (fraction[0] == b'.'
                && fraction.len() > 1
                && fraction[1..].iter().all(u8::is_ascii_digit)))
}

fn children(node: &Value) -> &[Value] {
    node.get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn node_type(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or("")
}

fn attr_u64(node: &Value, name: &str) -> Option<u64> {
    node.get("attrs")?.get(name)?.as_u64()
}

fn attr_str<'a>(node: &'a Value, name: &str) -> Option<&'a str> {
    node.get("attrs")?.get(name)?.as_str()
}

fn mark_types(node: &Value) -> Vec<&str> {
    node.get("marks")
        .and_then(Value::as_array)
        .map(|marks| {
            marks
                .iter()
                .filter_map(|mark| mark.get("type").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default()
}

fn link_href(node: &Value) -> Option<&str> {
    node.get("marks")?
        .as_array()?
        .iter()
        .find(|mark| mark.get("type").and_then(Value::as_str) == Some("link"))?
        .get("attrs")?
        .get("href")?
        .as_str()
}

// ---------------------------------------------------------------------------
// Testo semplice

pub fn to_plain_text(document: &Value) -> String {
    blocks_to_plain(children(document)).trim_end().to_owned()
}

fn blocks_to_plain(blocks: &[Value]) -> String {
    let rendered: Vec<String> = blocks
        .iter()
        .map(block_to_plain)
        .filter(|block| !block.is_empty())
        .collect();
    rendered.join("\n\n")
}

fn block_to_plain(node: &Value) -> String {
    match node_type(node) {
        "paragraph" | "heading" => inline_to_plain(children(node)),
        "codeBlock" => inline_to_plain(children(node)),
        "encryptedText" => ENCRYPTED_TEXT_PLACEHOLDER.to_owned(),
        "horizontalRule" => "----------".to_owned(),
        // Come il segnaposto degli allegati importati.
        "image" => {
            let notes = image_notes(node);
            if notes.is_empty() {
                format!("📎 {}", image_alt(node))
            } else {
                format!("📎 {} — {}", image_alt(node), notes.join("; "))
            }
        }
        "blockquote" => prefix_lines(&blocks_to_plain(children(node)), "> ", "> "),
        "bulletList" => list_to_plain(node, false),
        "orderedList" => list_to_plain(node, true),
        "taskList" => task_list(node, blocks_to_plain, "", ""),
        "table" => table_rows(node, |cell| inline_cell(cell, inline_to_plain))
            .into_iter()
            .map(|row| row.join(" | "))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => inline_to_plain(children(node)),
    }
}

fn is_checked(item: &Value) -> bool {
    item.get("attrs")
        .and_then(|attrs| attrs.get("checked"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Checklist come elenco con `[x]`/`[ ]`, comune a testo e Markdown.
fn task_list(node: &Value, render: fn(&[Value]) -> String, bullet: &str, indent: &str) -> String {
    children(node)
        .iter()
        .map(|item| {
            let marker = format!("{bullet}[{}] ", if is_checked(item) { "x" } else { " " });
            let rest = format!("{indent}{}", " ".repeat(marker.len() - indent.len()));
            let body = render(children(item));
            if body.is_empty() {
                marker.trim_end().to_owned()
            } else {
                prefix_lines(&body, &marker, &rest)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Celle di una tabella, riga per riga, già convertite.
fn table_rows(node: &Value, cell: impl Fn(&Value) -> String) -> Vec<Vec<String>> {
    children(node)
        .iter()
        .filter(|row| node_type(row) == "tableRow")
        .map(|row| children(row).iter().map(&cell).collect())
        .collect()
}

/// Contenuto di una cella su una riga sola (i paragrafi diventano spazi).
fn inline_cell(cell: &Value, render: fn(&[Value]) -> String) -> String {
    children(cell)
        .iter()
        .map(|block| render(children(block)))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\n', " ")
}

fn list_to_plain(node: &Value, ordered: bool) -> String {
    let start = attr_u64(node, "start").unwrap_or(1);
    children(node)
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let marker = if ordered {
                format!("{}. ", start + index as u64)
            } else {
                "- ".to_owned()
            };
            let indent = " ".repeat(marker.chars().count());
            prefix_lines(&blocks_to_plain(children(item)), &marker, &indent)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn inline_to_plain(nodes: &[Value]) -> String {
    let mut output = String::new();
    for node in nodes {
        match node_type(node) {
            "text" => output.push_str(node.get("text").and_then(Value::as_str).unwrap_or("")),
            "hardBreak" => output.push('\n'),
            _ => output.push_str(&inline_to_plain(children(node))),
        }
    }
    output
}

fn prefix_lines(text: &str, first: &str, rest: &str) -> String {
    let mut output = String::new();
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let prefix = if index == 0 { first } else { rest };
        if line.is_empty() {
            output.push_str(prefix.trim_end());
        } else {
            output.push_str(prefix);
            output.push_str(line);
        }
    }
    output
}

// ---------------------------------------------------------------------------
// Markdown

pub fn to_markdown(title: &str, document: &Value, front_matter: &FrontMatter) -> String {
    let mut output = String::new();
    let mut header = Vec::new();
    header.push(format!("title: {}", json_string(title)));
    if let Some(created) = &front_matter.created_at {
        header.push(format!("created: {}", json_string(created)));
    }
    if let Some(updated) = &front_matter.updated_at {
        header.push(format!("updated: {}", json_string(updated)));
    }
    if let Some(notebook) = &front_matter.notebook {
        header.push(format!("notebook: {}", json_string(notebook)));
    }
    if !front_matter.tags.is_empty() {
        header.push(format!("tags: {}", json!(front_matter.tags)));
    }
    if !front_matter.attachments.is_empty() {
        header.push(format!("attachments: {}", json!(front_matter.attachments)));
    }
    output.push_str("---\n");
    output.push_str(&header.join("\n"));
    output.push_str("\n---\n\n");
    output.push_str("# ");
    output.push_str(&escape_markdown_text(title.trim(), true));
    output.push_str("\n\n");
    IMAGE_PATHS.with(|paths| *paths.borrow_mut() = front_matter.image_paths.clone());
    let body = blocks_to_markdown(children(document));
    IMAGE_PATHS.with(|paths| paths.borrow_mut().clear());
    if !body.is_empty() {
        output.push_str(&body);
        output.push('\n');
    }
    output
}

fn json_string(value: &str) -> String {
    Value::String(value.to_owned()).to_string()
}

fn blocks_to_markdown(blocks: &[Value]) -> String {
    let rendered: Vec<String> = blocks
        .iter()
        .map(block_to_markdown)
        .filter(|block| !block.is_empty())
        .collect();
    rendered.join("\n\n")
}

fn block_to_markdown(node: &Value) -> String {
    match node_type(node) {
        "paragraph" => inline_to_markdown(children(node)),
        "heading" => {
            let level = attr_u64(node, "level").unwrap_or(1).clamp(1, 6) as usize;
            let text = inline_to_markdown(children(node));
            if text.is_empty() {
                String::new()
            } else {
                format!("{} {}", "#".repeat(level), text.replace('\n', " "))
            }
        }
        "codeBlock" => {
            let code = inline_to_plain(children(node));
            let fence = "`".repeat(longest_run(&code, '`').max(2) + 1);
            let language = attr_str(node, "language")
                .filter(|language| {
                    language
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '.'))
                })
                .unwrap_or("");
            format!("{fence}{language}\n{code}\n{fence}")
        }
        "encryptedText" => format!(
            "```{ENCRYPTED_TEXT_LANGUAGE}\n{}\n```",
            ciphertext(node).unwrap_or_default()
        ),
        "horizontalRule" => "---".to_owned(),
        "image" => {
            let alt = escape_markdown_text(&image_alt(node), false).replace(['[', ']'], "");
            let path = attr_str(node, "attachmentId")
                .and_then(|id| IMAGE_PATHS.with(|paths| paths.borrow().get(id).cloned()));
            let image = match path {
                Some(path) => format!("![{alt}](<{path}>)"),
                None => format!("*[{alt}]*"),
            };
            let notes = image_notes(node);
            if notes.is_empty() {
                image
            } else {
                let notes: Vec<String> = notes
                    .iter()
                    .map(|note| escape_markdown_text(note, false))
                    .collect();
                format!("{image}\n\n*{}*", notes.join(" · "))
            }
        }
        "blockquote" => prefix_lines(&blocks_to_markdown(children(node)), "> ", "> "),
        "bulletList" => markdown_list(node, false),
        "orderedList" => markdown_list(node, true),
        "taskList" => task_list(node, blocks_to_markdown, "- ", "  "),
        "table" => markdown_table(node),
        _ => inline_to_markdown(children(node)),
    }
}

/// Tabella GFM: la prima riga fa da intestazione.
fn markdown_table(node: &Value) -> String {
    // `inline_to_markdown` protegge già `|` nel testo delle celle.
    let rows = table_rows(node, |cell| inline_cell(cell, inline_to_markdown));
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return String::new();
    }
    let line = |row: &[String]| {
        let mut cells: Vec<&str> = row.iter().map(String::as_str).collect();
        cells.resize(columns, "");
        format!("| {} |", cells.join(" | "))
    };
    let mut output = vec![line(&rows[0]), format!("|{}", " --- |".repeat(columns))];
    output.extend(rows[1..].iter().map(|row| line(row)));
    output.join("\n")
}

fn markdown_list(node: &Value, ordered: bool) -> String {
    let start = attr_u64(node, "start").unwrap_or(1);
    children(node)
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let marker = if ordered {
                format!("{}. ", start + index as u64)
            } else {
                "- ".to_owned()
            };
            let indent = " ".repeat(marker.len());
            let body = blocks_to_markdown(children(item));
            if body.is_empty() {
                marker.trim_end().to_owned()
            } else {
                prefix_lines(&body, &marker, &indent)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn longest_run(text: &str, character: char) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for c in text.chars() {
        if c == character {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

fn escape_markdown_text(text: &str, at_line_start: bool) -> String {
    let mut output = String::with_capacity(text.len());
    let mut line_start = at_line_start;
    let mut leading_digits = false;
    for character in text.chars() {
        let escape = match character {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '~' | '&' | '|' => true,
            '#' | '-' | '+' | '=' => line_start,
            '.' | ')' => leading_digits,
            _ => false,
        };
        if escape {
            output.push('\\');
        }
        output.push(character);
        leading_digits = (line_start || leading_digits) && character.is_ascii_digit();
        line_start = false;
    }
    output
}

fn inline_to_markdown(nodes: &[Value]) -> String {
    let mut output = String::new();
    let mut open: Vec<String> = Vec::new();
    let mut open_href: Option<String> = None;
    let mut at_line_start = true;

    for node in nodes {
        match node_type(node) {
            "hardBreak" => {
                close_marks(&mut output, &mut open, &mut open_href, 0);
                output.push_str("\\\n");
                at_line_start = true;
            }
            "text" => {
                let text = node.get("text").and_then(Value::as_str).unwrap_or("");
                if text.is_empty() {
                    continue;
                }
                let mut wanted: Vec<String> = MARK_ORDER
                    .iter()
                    .filter(|mark| mark_types(node).contains(mark))
                    .map(|mark| (*mark).to_owned())
                    .collect();
                let href = link_href(node).filter(|href| is_safe_link(href));
                if href.is_none() {
                    wanted.retain(|mark| mark != "link");
                }
                let is_code = wanted.iter().any(|mark| mark == "code");
                wanted.retain(|mark| mark != "code");

                // Le marcature aperte restano valide solo se coincidono come
                // prefisso (e, per i link, con la stessa destinazione).
                let mut keep = 0;
                while keep < open.len()
                    && keep < wanted.len()
                    && open[keep] == wanted[keep]
                    && (open[keep] != "link" || open_href.as_deref() == href)
                {
                    keep += 1;
                }
                if text.trim().is_empty() && !is_code {
                    close_marks(&mut output, &mut open, &mut open_href, keep);
                    output.push_str(&escape_markdown_text(text, at_line_start));
                    continue;
                }
                close_marks(&mut output, &mut open, &mut open_href, keep);

                let leading = &text[..text.len() - text.trim_start().len()];
                let core = text.trim();
                let trailing = &text[text.trim_end().len()..];
                output.push_str(&escape_markdown_text(leading, at_line_start));
                if !leading.is_empty() {
                    at_line_start = false;
                }
                for mark in &wanted[keep..] {
                    output.push_str(match mark.as_str() {
                        "link" => "[",
                        "bold" => "**",
                        "italic" => "*",
                        "strike" => "~~",
                        _ => "<u>",
                    });
                    if mark == "link" {
                        open_href = href.map(str::to_owned);
                    }
                    open.push(mark.clone());
                }
                if is_code {
                    let fence = "`".repeat(longest_run(core, '`') + 1);
                    let padding = if core.starts_with('`') || core.ends_with('`') {
                        " "
                    } else {
                        ""
                    };
                    output.push_str(&format!("{fence}{padding}{core}{padding}{fence}"));
                } else {
                    output.push_str(&escape_markdown_text(
                        core,
                        at_line_start && open.is_empty(),
                    ));
                }
                at_line_start = false;
                if !trailing.is_empty() {
                    close_marks(&mut output, &mut open, &mut open_href, 0);
                    output.push_str(trailing);
                }
            }
            _ => output.push_str(&inline_to_markdown(children(node))),
        }
    }
    close_marks(&mut output, &mut open, &mut open_href, 0);
    output
}

fn close_marks(
    output: &mut String,
    open: &mut Vec<String>,
    open_href: &mut Option<String>,
    keep: usize,
) {
    while open.len() > keep {
        let Some(mark) = open.pop() else { break };
        match mark.as_str() {
            "link" => {
                let href = open_href.take().unwrap_or_default();
                output.push_str(&format!(
                    "]({})",
                    href.replace('(', "%28").replace(')', "%29")
                ));
            }
            "bold" => output.push_str("**"),
            "italic" => output.push('*'),
            "strike" => output.push_str("~~"),
            _ => output.push_str("</u>"),
        }
    }
}

// ---------------------------------------------------------------------------
// HTML sicuro

pub fn escape_html(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
    output
}

/// Documento HTML autonomo. Il testo è sempre codificato; la CSP impedisce
/// script e risorse remote anche se il file viene aperto in un browser.
pub fn to_html(title: &str, document: &Value, language: &str) -> String {
    let language = if language
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c == '-')
    {
        language
    } else {
        "it"
    };
    format!(
        "<!doctype html>\n<html lang=\"{language}\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:\">\n\
         <title>{title}</title>\n<style>{style}</style>\n</head>\n<body>\n<article>\n<h1>{title}</h1>\n{body}</article>\n</body>\n</html>\n",
        title = escape_html(title),
        style = HTML_STYLE,
        body = blocks_to_html(children(document)),
    )
}

const HTML_STYLE: &str = "body{font-family:system-ui,sans-serif;line-height:1.6;color:#262921;\
background:#fff;margin:0;padding:2rem 1rem}article{max-width:46rem;margin:0 auto}\
pre{background:#f4f3ef;padding:.75rem;overflow:auto}code{font-family:ui-monospace,monospace}\
blockquote{border-left:3px solid #c9cbc4;margin-left:0;padding-left:1rem;color:#60665d}\
table{border-collapse:collapse}td,th{border:1px solid #c9cbc4;padding:.3rem .5rem}\
ul.checklist{list-style:none;padding-left:.5rem}\
@media print{body{padding:0}}";

fn blocks_to_html(blocks: &[Value]) -> String {
    let mut output = String::new();
    for node in blocks {
        match node_type(node) {
            "paragraph" => {
                output.push_str(&format!("<p>{}</p>\n", inline_to_html(children(node))));
            }
            "heading" => {
                let level = attr_u64(node, "level").unwrap_or(1).clamp(1, 6);
                // h1 è riservato al titolo della nota.
                let level = (level + 1).min(6);
                output.push_str(&format!(
                    "<h{level}>{}</h{level}>\n",
                    inline_to_html(children(node))
                ));
            }
            "codeBlock" => output.push_str(&format!(
                "<pre><code>{}</code></pre>\n",
                escape_html(&inline_to_plain(children(node)))
            )),
            "encryptedText" => output.push_str(&format!(
                "<pre><code class=\"language-{ENCRYPTED_TEXT_LANGUAGE}\">{}</code></pre>\n",
                escape_html(ciphertext(node).unwrap_or_default())
            )),
            "horizontalRule" => output.push_str("<hr>\n"),
            "image" => {
                output.push_str(&format!(
                    "<p class=\"image\">[{}]</p>\n",
                    escape_html(&image_alt(node))
                ));
                let notes = image_notes(node);
                if !notes.is_empty() {
                    output.push_str(&format!(
                        "<p class=\"image-notes\"><em>{}</em></p>\n",
                        escape_html(&notes.join(" · "))
                    ));
                }
            }
            "blockquote" => output.push_str(&format!(
                "<blockquote>\n{}</blockquote>\n",
                blocks_to_html(children(node))
            )),
            "bulletList" => output.push_str(&format!("<ul>\n{}</ul>\n", list_items_html(node))),
            "orderedList" => {
                let start = attr_u64(node, "start").unwrap_or(1);
                let start = if start == 1 {
                    String::new()
                } else {
                    format!(" start=\"{start}\"")
                };
                output.push_str(&format!("<ol{start}>\n{}</ol>\n", list_items_html(node)));
            }
            "taskList" => {
                output.push_str("<ul class=\"checklist\">\n");
                for item in children(node) {
                    let checked = if is_checked(item) { " checked" } else { "" };
                    output.push_str(&format!(
                        "<li><input type=\"checkbox\" disabled{checked}> {}</li>\n",
                        blocks_to_html(children(item)).trim_end()
                    ));
                }
                output.push_str("</ul>\n");
            }
            "table" => {
                output.push_str("<table>\n");
                for row in children(node)
                    .iter()
                    .filter(|row| node_type(row) == "tableRow")
                {
                    output.push_str("<tr>");
                    for cell in children(row) {
                        let tag = if node_type(cell) == "tableHeader" {
                            "th"
                        } else {
                            "td"
                        };
                        output.push_str(&format!(
                            "<{tag}>{}</{tag}>",
                            blocks_to_html(children(cell)).trim_end()
                        ));
                    }
                    output.push_str("</tr>\n");
                }
                output.push_str("</table>\n");
            }
            _ => output.push_str(&format!("<p>{}</p>\n", inline_to_html(children(node)))),
        }
    }
    output
}

fn list_items_html(node: &Value) -> String {
    children(node)
        .iter()
        .map(|item| format!("<li>{}</li>\n", blocks_to_html(children(item)).trim_end()))
        .collect()
}

fn inline_to_html(nodes: &[Value]) -> String {
    let mut output = String::new();
    for node in nodes {
        match node_type(node) {
            "hardBreak" => output.push_str("<br>"),
            "text" => {
                let mut html = escape_html(node.get("text").and_then(Value::as_str).unwrap_or(""));
                let marks = mark_types(node);
                for (mark, open, close) in [
                    ("code", "<code>", "</code>"),
                    ("underline", "<u>", "</u>"),
                    ("strike", "<s>", "</s>"),
                    ("italic", "<em>", "</em>"),
                    ("bold", "<strong>", "</strong>"),
                ] {
                    if marks.contains(&mark) {
                        html = format!("{open}{html}{close}");
                    }
                }
                if let Some(style) = text_style_css(node) {
                    html = format!("<span style=\"{style}\">{html}</span>");
                }
                if marks.contains(&"highlight") {
                    html = match mark_attr(node, "highlight", "color").filter(|c| is_hex_color(c)) {
                        Some(color) => format!("<mark style=\"background:{color}\">{html}</mark>"),
                        None => format!("<mark>{html}</mark>"),
                    };
                }
                if let Some(href) = link_href(node).filter(|href| is_safe_link(href)) {
                    html = format!(
                        "<a href=\"{}\" rel=\"noopener noreferrer\">{html}</a>",
                        escape_html(href)
                    );
                }
                output.push_str(&html);
            }
            _ => output.push_str(&inline_to_html(children(node))),
        }
    }
    output
}

fn mark_attr<'a>(node: &'a Value, mark_type: &str, name: &str) -> Option<&'a str> {
    node.get("marks")?
        .as_array()?
        .iter()
        .find(|mark| mark.get("type").and_then(Value::as_str) == Some(mark_type))?
        .get("attrs")?
        .get(name)?
        .as_str()
}

/// Stile CSS di `textStyle`, solo con valori nella whitelist della
/// validazione: nessun testo arbitrario finisce nell'attributo `style`.
fn text_style_css(node: &Value) -> Option<String> {
    let mut rules = Vec::new();
    if let Some(color) = mark_attr(node, "textStyle", "color").filter(|c| is_hex_color(c)) {
        rules.push(format!("color:{color}"));
    }
    if let Some(family) =
        mark_attr(node, "textStyle", "fontFamily").filter(|family| FONT_FAMILIES.contains(family))
    {
        rules.push(format!("font-family:{family}"));
    }
    if let Some(size) =
        mark_attr(node, "textStyle", "fontSize").filter(|size| FONT_SIZES.contains(size))
    {
        rules.push(format!("font-size:{size}"));
    }
    (!rules.is_empty()).then(|| rules.join(";"))
}

// ---------------------------------------------------------------------------
// Costruzione di documenti importati

struct Frame {
    kind: &'static str,
    attrs: Option<Value>,
    content: Vec<Value>,
}

/// Costruttore che garantisce la struttura richiesta da Tiptap: testo solo in
/// blocchi testuali, liste composte da elementi e annidamento limitato.
struct DocumentBuilder {
    stack: Vec<Frame>,
    marks: Vec<Value>,
    collapse_whitespace: bool,
}

fn is_textblock(kind: &str) -> bool {
    matches!(kind, "paragraph" | "heading" | "codeBlock")
}

fn is_list(kind: &str) -> bool {
    matches!(kind, "bulletList" | "orderedList")
}

impl DocumentBuilder {
    fn new(collapse_whitespace: bool) -> Self {
        Self {
            stack: vec![Frame {
                kind: "doc",
                attrs: None,
                content: Vec::new(),
            }],
            marks: Vec::new(),
            collapse_whitespace,
        }
    }

    fn top_kind(&self) -> &'static str {
        self.stack.last().map(|frame| frame.kind).unwrap_or("doc")
    }

    fn push_frame(&mut self, kind: &'static str, attrs: Option<Value>) {
        self.stack.push(Frame {
            kind,
            attrs,
            content: Vec::new(),
        });
    }

    /// Apre un blocco e restituisce il livello da passare a `end_to`. `None`
    /// indica che il blocco è stato reso trasparente per limiti di profondità.
    fn start_block(&mut self, kind: &'static str, attrs: Option<Value>) -> Option<usize> {
        while is_textblock(self.top_kind()) {
            self.end_top();
        }
        let level = self.stack.len();
        let needs_item = is_list(self.top_kind()) && kind != "listItem";
        let needs_list = kind == "listItem" && !is_list(self.top_kind());
        let extra = usize::from(needs_item || needs_list);
        if !is_textblock(kind) && kind != "horizontalRule" && level + extra >= MAX_BLOCK_STACK {
            return None;
        }
        if needs_item {
            self.push_frame("listItem", None);
        }
        if needs_list {
            self.push_frame("bulletList", None);
        }
        self.push_frame(kind, attrs);
        Some(level)
    }

    fn end_to(&mut self, level: usize) {
        while self.stack.len() > level.max(1) {
            self.end_top();
        }
    }

    fn end_top(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let Some(frame) = self.stack.pop() else {
            return;
        };
        if let Some(node) = finish_frame(frame)
            && let Some(parent) = self.stack.last_mut()
        {
            parent.content.push(node);
        }
    }

    fn ensure_inline_container(&mut self) {
        if is_textblock(self.top_kind()) {
            return;
        }
        if is_list(self.top_kind()) {
            self.push_frame("listItem", None);
        }
        self.push_frame("paragraph", None);
    }

    fn add_text(&mut self, text: &str) {
        let in_code = self.top_kind() == "codeBlock";
        let mut text = if self.collapse_whitespace && !in_code {
            collapse_whitespace(text)
        } else {
            text.to_owned()
        };
        if text.is_empty() {
            return;
        }
        if !is_textblock(self.top_kind()) && text.trim().is_empty() {
            return;
        }
        self.ensure_inline_container();
        let Some(frame) = self.stack.last_mut() else {
            return;
        };
        if in_code {
            append_text(&mut frame.content, text, Vec::new());
            return;
        }
        let previous_ends_with_space = match frame.content.last() {
            None => true,
            Some(last) if node_type(last) == "hardBreak" => true,
            Some(last) => last
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|value| value.ends_with(' ')),
        };
        if self.collapse_whitespace && previous_ends_with_space {
            text = text.trim_start().to_owned();
        }
        if frame.content.is_empty() {
            text = text.trim_start().to_owned();
        }
        if text.is_empty() {
            return;
        }
        append_text(&mut frame.content, text, sorted_marks(&self.marks));
    }

    fn hard_break(&mut self) {
        if self.top_kind() == "codeBlock" {
            self.add_text("\n");
            return;
        }
        self.ensure_inline_container();
        if let Some(frame) = self.stack.last_mut()
            && !frame.content.is_empty()
        {
            trim_trailing_space(&mut frame.content);
            frame.content.push(json!({ "type": "hardBreak" }));
        }
    }

    fn rule(&mut self) {
        if let Some(level) = self.start_block("horizontalRule", None) {
            self.end_to(level);
        }
    }

    fn push_mark(&mut self, mark: Value) {
        self.marks.push(mark);
    }

    fn pop_mark(&mut self) {
        self.marks.pop();
    }

    fn remove_last_mark(&mut self, mark_type: &str) {
        if let Some(index) = self
            .marks
            .iter()
            .rposition(|mark| mark.get("type").and_then(Value::as_str) == Some(mark_type))
        {
            self.marks.remove(index);
        }
    }

    fn finish(mut self) -> Value {
        self.end_to(1);
        let content = self
            .stack
            .pop()
            .map(|frame| frame.content)
            .unwrap_or_default();
        if content.is_empty() {
            return crate::domain::note::empty_document();
        }
        json!({ "type": "doc", "content": content })
    }
}

fn collapse_whitespace(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut previous_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if !previous_space {
                output.push(' ');
            }
            previous_space = true;
        } else {
            output.push(character);
            previous_space = false;
        }
    }
    output
}

fn sorted_marks(marks: &[Value]) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();
    for name in MARK_ORDER {
        if let Some(mark) = marks
            .iter()
            .rev()
            .find(|mark| mark.get("type").and_then(Value::as_str) == Some(name))
        {
            result.push(mark.clone());
        }
    }
    result
}

fn append_text(content: &mut Vec<Value>, text: String, marks: Vec<Value>) {
    if let Some(last) = content.last_mut()
        && node_type(last) == "text"
        && last
            .get("marks")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            == marks.as_slice()
        && let Some(existing) = last.get("text").and_then(Value::as_str)
    {
        let merged = format!("{existing}{text}");
        last["text"] = Value::String(merged);
        return;
    }
    let mut node = Map::new();
    node.insert("type".to_owned(), Value::String("text".to_owned()));
    node.insert("text".to_owned(), Value::String(text));
    if !marks.is_empty() {
        node.insert("marks".to_owned(), Value::Array(marks));
    }
    content.push(Value::Object(node));
}

fn trim_trailing_space(content: &mut Vec<Value>) {
    while let Some(last) = content.last_mut() {
        if node_type(last) == "hardBreak" {
            content.pop();
            continue;
        }
        let Some(text) = last.get("text").and_then(Value::as_str) else {
            return;
        };
        let trimmed = text.trim_end().to_owned();
        if trimmed.is_empty() {
            content.pop();
        } else {
            last["text"] = Value::String(trimmed);
            return;
        }
    }
}

fn finish_frame(frame: Frame) -> Option<Value> {
    let Frame {
        kind,
        attrs,
        mut content,
    } = frame;
    match kind {
        "paragraph" | "heading" => {
            trim_trailing_space(&mut content);
            if content.is_empty() {
                return None;
            }
        }
        "codeBlock" => {
            if let Some(last) = content.last_mut()
                && let Some(text) = last.get("text").and_then(Value::as_str)
            {
                let trimmed = text.strip_suffix('\n').unwrap_or(text).to_owned();
                if trimmed.is_empty() {
                    content.clear();
                } else {
                    last["text"] = Value::String(trimmed);
                }
            }
            // Un blocco `age` con un testo cifrato valido (esportato
            // dall'app o creato con `age -p -a`) torna un testo cifrato.
            let language = attrs
                .as_ref()
                .and_then(|attrs| attrs.get("language"))
                .and_then(Value::as_str);
            if language == Some(ENCRYPTED_TEXT_LANGUAGE)
                && let [text] = content.as_slice()
                && let Some(text) = text.get("text").and_then(Value::as_str)
                && is_armored_ciphertext(text.trim())
            {
                return Some(json!({
                    "type": "encryptedText",
                    "attrs": { "ciphertext": text.trim(), "hint": null },
                }));
            }
        }
        "listItem" => {
            if content.first().map(node_type) != Some("paragraph") {
                content.insert(0, json!({ "type": "paragraph" }));
            }
        }
        "bulletList" | "orderedList" | "blockquote" if content.is_empty() => {
            return None;
        }
        _ => {}
    }
    let mut node = Map::new();
    node.insert("type".to_owned(), Value::String(kind.to_owned()));
    if let Some(attrs) = attrs {
        node.insert("attrs".to_owned(), attrs);
    }
    if !content.is_empty() {
        node.insert("content".to_owned(), Value::Array(content));
    }
    Some(Value::Object(node))
}

/// Estrae il primo titolo di livello 1 se apre il documento.
fn take_leading_title(document: &mut Value, expected: Option<&str>) -> Option<String> {
    let blocks = document.get_mut("content")?.as_array_mut()?;
    let first = blocks.first()?;
    if node_type(first) != "heading" || attr_u64(first, "level") != Some(1) {
        return None;
    }
    let text = inline_to_plain(children(first)).trim().to_owned();
    if expected.is_some_and(|expected| expected.trim() != text) {
        return None;
    }
    blocks.remove(0);
    if blocks.is_empty() {
        blocks.push(json!({ "type": "paragraph" }));
    }
    Some(text)
}

pub fn text_to_document(text: &str) -> Value {
    let mut builder = DocumentBuilder::new(false);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    for paragraph in normalized.split("\n\n") {
        if paragraph.trim().is_empty() {
            continue;
        }
        if let Some(level) = builder.start_block("paragraph", None) {
            for (index, line) in paragraph.trim_matches('\n').split('\n').enumerate() {
                if index > 0 {
                    builder.hard_break();
                }
                builder.add_text(line);
            }
            builder.end_to(level);
        }
    }
    builder.finish()
}

#[derive(Clone, Copy)]
enum MarkdownEntry {
    Block(Option<usize>),
    Mark,
    Transparent,
    Metadata,
    HtmlBlock,
}

pub fn markdown_to_document(markdown: &str) -> ParsedDocument {
    let options = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut builder = DocumentBuilder::new(false);
    let mut entries: Vec<MarkdownEntry> = Vec::new();
    let mut metadata = String::new();
    let mut html_block = String::new();
    let mut in_metadata = false;
    let mut in_html_block = false;

    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(tag) => {
                let entry = match tag {
                    Tag::Paragraph => MarkdownEntry::Block(builder.start_block("paragraph", None)),
                    Tag::Heading { level, .. } => {
                        let level = heading_level(level);
                        MarkdownEntry::Block(
                            builder.start_block("heading", Some(json!({ "level": level }))),
                        )
                    }
                    Tag::BlockQuote(_) => {
                        MarkdownEntry::Block(builder.start_block("blockquote", None))
                    }
                    Tag::CodeBlock(kind) => {
                        let language = match kind {
                            CodeBlockKind::Fenced(info) => info
                                .split_whitespace()
                                .next()
                                .filter(|language| language.len() <= 40)
                                .map(str::to_owned),
                            CodeBlockKind::Indented => None,
                        };
                        MarkdownEntry::Block(
                            builder.start_block("codeBlock", Some(json!({ "language": language }))),
                        )
                    }
                    Tag::List(Some(start)) => MarkdownEntry::Block(
                        builder.start_block("orderedList", Some(json!({ "start": start }))),
                    ),
                    Tag::List(None) => {
                        MarkdownEntry::Block(builder.start_block("bulletList", None))
                    }
                    Tag::Item => MarkdownEntry::Block(builder.start_block("listItem", None)),
                    Tag::TableHead | Tag::TableRow => {
                        MarkdownEntry::Block(builder.start_block("paragraph", None))
                    }
                    Tag::TableCell => {
                        let has_content = builder.stack.last().is_some_and(|frame| {
                            frame.kind == "paragraph" && !frame.content.is_empty()
                        });
                        if has_content {
                            builder.add_text(" | ");
                        }
                        MarkdownEntry::Transparent
                    }
                    Tag::Emphasis => {
                        builder.push_mark(json!({ "type": "italic" }));
                        MarkdownEntry::Mark
                    }
                    Tag::Strong => {
                        builder.push_mark(json!({ "type": "bold" }));
                        MarkdownEntry::Mark
                    }
                    Tag::Strikethrough => {
                        builder.push_mark(json!({ "type": "strike" }));
                        MarkdownEntry::Mark
                    }
                    Tag::Link { dest_url, .. } if is_safe_link(&dest_url) => {
                        builder.push_mark(link_mark(dest_url.trim()));
                        MarkdownEntry::Mark
                    }
                    Tag::MetadataBlock(_) => {
                        in_metadata = true;
                        MarkdownEntry::Metadata
                    }
                    Tag::HtmlBlock => {
                        in_html_block = true;
                        html_block.clear();
                        MarkdownEntry::HtmlBlock
                    }
                    _ => MarkdownEntry::Transparent,
                };
                entries.push(entry);
            }
            Event::End(_) => {
                let entry = entries.pop().unwrap_or(MarkdownEntry::Transparent);
                match entry {
                    MarkdownEntry::Block(Some(level)) => builder.end_to(level),
                    MarkdownEntry::Block(None) | MarkdownEntry::Transparent => {}
                    MarkdownEntry::Mark => builder.pop_mark(),
                    MarkdownEntry::Metadata => in_metadata = false,
                    MarkdownEntry::HtmlBlock => {
                        in_html_block = false;
                        let fragment = Html::parse_fragment(&html_block);
                        builder.collapse_whitespace = true;
                        walk_html_children(fragment.root_element(), &mut builder, 0);
                        builder.collapse_whitespace = false;
                    }
                }
            }
            Event::Text(text) => {
                if in_metadata {
                    metadata.push_str(&text);
                } else if in_html_block {
                    html_block.push_str(&text);
                } else {
                    builder.add_text(&text);
                }
            }
            Event::Code(code) => {
                builder.push_mark(json!({ "type": "code" }));
                builder.add_text(&code);
                builder.pop_mark();
            }
            Event::Html(html) => {
                if in_html_block {
                    html_block.push_str(&html);
                }
            }
            Event::InlineHtml(html) => {
                let tag = html.trim().to_ascii_lowercase().replace(' ', "");
                match tag.as_str() {
                    "<u>" | "<ins>" => builder.push_mark(json!({ "type": "underline" })),
                    "</u>" | "</ins>" => builder.remove_last_mark("underline"),
                    "<br>" | "<br/>" => builder.hard_break(),
                    _ => {}
                }
            }
            Event::SoftBreak => builder.add_text(" "),
            Event::HardBreak => builder.hard_break(),
            Event::Rule => builder.rule(),
            Event::TaskListMarker(checked) => {
                builder.add_text(if checked { "☑ " } else { "☐ " })
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => builder.add_text(&text),
            Event::FootnoteReference(label) => builder.add_text(&format!("[^{label}]")),
        }
    }

    let front_matter = parse_front_matter(&metadata);
    let mut document = builder.finish();
    let title = match front_matter.title.as_deref() {
        Some(title) => {
            take_leading_title(&mut document, Some(title));
            Some(title.to_owned())
        }
        None => take_leading_title(&mut document, None),
    };
    ParsedDocument {
        title,
        document,
        front_matter,
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn link_mark(href: &str) -> Value {
    json!({ "type": "link", "attrs": { "href": href } })
}

fn parse_front_matter(source: &str) -> FrontMatter {
    let mut front_matter = FrontMatter::default();
    for line in source.lines().take(200) {
        let Some((key, raw)) = line.split_once(':') else {
            continue;
        };
        let raw = raw.trim();
        let parsed: Option<Value> = serde_json::from_str(raw).ok();
        let as_string = || -> Option<String> {
            match &parsed {
                Some(Value::String(value)) => Some(value.clone()),
                Some(_) => None,
                None => {
                    let value = raw.trim_matches(|c| c == '"' || c == '\'').trim();
                    (!value.is_empty()).then(|| value.to_owned())
                }
            }
        };
        let as_list = || -> Vec<String> {
            match &parsed {
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect(),
                Some(Value::String(value)) => vec![value.clone()],
                Some(_) => Vec::new(),
                None => raw
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(|item| {
                        item.trim()
                            .trim_matches(|c| c == '"' || c == '\'')
                            .trim()
                            .to_owned()
                    })
                    .filter(|item| !item.is_empty())
                    .collect(),
            }
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "title" => front_matter.title = as_string().filter(|title| !title.trim().is_empty()),
            "created" => {
                front_matter.created_at = as_string().filter(|value| is_utc_timestamp(value))
            }
            "updated" => {
                front_matter.updated_at = as_string().filter(|value| is_utc_timestamp(value))
            }
            "notebook" => front_matter.notebook = as_string(),
            "tags" => front_matter.tags = as_list().into_iter().take(100).collect(),
            "attachments" => front_matter.attachments = as_list().into_iter().take(200).collect(),
            _ => {}
        }
    }
    front_matter
}

pub fn html_to_document(source: &str) -> ParsedDocument {
    let html = Html::parse_document(source);
    let mut builder = DocumentBuilder::new(true);
    let root = html.root_element();
    let mut page_title = None;
    for child in root.children() {
        let Some(element) = ElementRef::wrap(child) else {
            continue;
        };
        if element.value().name() == "head" {
            page_title = element
                .children()
                .filter_map(ElementRef::wrap)
                .find(|element| element.value().name() == "title")
                .map(|title| {
                    collapse_whitespace(&title.text().collect::<String>())
                        .trim()
                        .to_owned()
                })
                .filter(|title| !title.is_empty());
        } else {
            walk_html_element(element, &mut builder, 0);
        }
    }
    let mut document = builder.finish();
    let leading = take_leading_title(&mut document, page_title.as_deref());
    ParsedDocument {
        title: page_title.or(leading),
        document,
        front_matter: FrontMatter::default(),
    }
}

fn walk_html_children(element: ElementRef<'_>, builder: &mut DocumentBuilder, depth: usize) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => builder.add_text(text),
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    walk_html_element(child, builder, depth + 1);
                }
            }
            _ => {}
        }
    }
}

fn walk_html_element(element: ElementRef<'_>, builder: &mut DocumentBuilder, depth: usize) {
    if depth > MAX_HTML_DEPTH {
        // Oltre il limite il sottoalbero viene appiattito senza ricorsione.
        let text: String = element.text().collect::<Vec<_>>().join(" ");
        builder.add_text(&text);
        return;
    }
    let name = element.value().name().to_ascii_lowercase();
    let block = |builder: &mut DocumentBuilder, kind: &'static str, attrs: Option<Value>| {
        let level = builder.start_block(kind, attrs);
        walk_html_children(element, builder, depth);
        if let Some(level) = level {
            builder.end_to(level);
        }
    };
    let mark = |builder: &mut DocumentBuilder, mark: Value| {
        builder.push_mark(mark);
        walk_html_children(element, builder, depth);
        builder.pop_mark();
    };
    match name.as_str() {
        "script" | "style" | "template" | "noscript" | "iframe" | "object" | "embed" | "svg"
        | "math" | "head" | "title" | "meta" | "link" | "base" | "input" | "select"
        | "textarea" | "button" | "canvas" | "audio" | "video" | "source" | "track" | "frame"
        | "frameset" | "applet" | "dialog" => {}
        "p" => block(builder, "paragraph", None),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = name[1..].parse::<u8>().unwrap_or(1);
            block(builder, "heading", Some(json!({ "level": level })));
        }
        "ul" | "menu" => block(builder, "bulletList", None),
        "ol" => {
            let start = element
                .value()
                .attr("start")
                .and_then(|start| start.trim().parse::<u64>().ok())
                .filter(|start| *start < 1_000_000)
                .unwrap_or(1);
            block(builder, "orderedList", Some(json!({ "start": start })));
        }
        "li" => block(builder, "listItem", None),
        "blockquote" => block(builder, "blockquote", None),
        "pre" => {
            let language = element
                .children()
                .filter_map(ElementRef::wrap)
                .find(|child| child.value().name() == "code")
                .and_then(|code| code.value().attr("class"))
                .and_then(|classes| {
                    classes
                        .split_whitespace()
                        .find_map(|class| class.strip_prefix("language-"))
                        .filter(|language| {
                            language.len() <= 40
                                && language.chars().all(|c| {
                                    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+')
                                })
                        })
                        .map(str::to_owned)
                });
            let level = builder.start_block("codeBlock", Some(json!({ "language": language })));
            let text: String = element.text().collect();
            builder.add_text(text.strip_prefix('\n').unwrap_or(&text));
            if let Some(level) = level {
                builder.end_to(level);
            }
        }
        "hr" => builder.rule(),
        "br" => builder.hard_break(),
        "strong" | "b" => mark(builder, json!({ "type": "bold" })),
        "em" | "i" | "cite" | "dfn" => mark(builder, json!({ "type": "italic" })),
        "s" | "strike" | "del" => mark(builder, json!({ "type": "strike" })),
        "u" | "ins" => mark(builder, json!({ "type": "underline" })),
        "code" | "kbd" | "samp" | "tt" => mark(builder, json!({ "type": "code" })),
        "a" => match element
            .value()
            .attr("href")
            .filter(|href| is_safe_link(href))
        {
            Some(href) => mark(builder, link_mark(href.trim())),
            None => walk_html_children(element, builder, depth),
        },
        "img" => {
            if let Some(alt) = element
                .value()
                .attr("alt")
                .filter(|alt| !alt.trim().is_empty())
            {
                builder.add_text(&format!(" [{}] ", alt.trim()));
            }
        }
        "tr" => block(builder, "paragraph", None),
        "td" | "th" => {
            let has_content = builder
                .stack
                .last()
                .is_some_and(|frame| frame.kind == "paragraph" && !frame.content.is_empty());
            if has_content {
                builder.add_text(" | ");
            }
            walk_html_children(element, builder, depth);
        }
        "div" | "section" | "article" | "main" | "header" | "footer" | "nav" | "aside"
        | "figure" | "figcaption" | "address" | "details" | "summary" | "dl" | "dt" | "dd"
        | "table" | "thead" | "tbody" | "tfoot" | "caption" | "fieldset" | "legend" | "form"
        | "center" | "hgroup" | "body" | "html" => {
            // Contenitori neutri: chiudono il paragrafo implicito corrente.
            while is_textblock(builder.top_kind()) && builder.top_kind() == "paragraph" {
                let level = builder.stack.len() - 1;
                builder.end_to(level);
            }
            walk_html_children(element, builder, depth);
            while builder.top_kind() == "paragraph" {
                let level = builder.stack.len() - 1;
                builder.end_to(level);
            }
        }
        _ => walk_html_children(element, builder, depth),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn images_reference_attachments_only_and_export_to_markdown() {
        use crate::domain::note::validate_document;
        let image =
            |attrs: Value| json!({"type": "doc", "content": [{"type": "image", "attrs": attrs}]});
        let valid = image(json!({"attachmentId": "abc-123", "alt": "Schema", "width": 320}));
        validate_document(&valid).expect("valid image");
        for invalid in [
            json!({"attachmentId": "abc", "src": "https://example.com/x.png"}),
            json!({"attachmentId": "../etc/passwd"}),
            json!({"alt": "senza allegato"}),
            json!({"attachmentId": "abc", "width": 5}),
        ] {
            assert!(
                validate_document(&image(invalid.clone())).is_err(),
                "{invalid}"
            );
        }
        let front = FrontMatter {
            image_paths: std::collections::HashMap::from([(
                "abc-123".to_owned(),
                "attachments/Nota/schema 1.png".to_owned(),
            )]),
            ..FrontMatter::default()
        };
        let markdown = to_markdown("Nota", &valid, &front);
        assert!(
            markdown.contains("![Schema](<attachments/Nota/schema 1.png>)"),
            "{markdown}"
        );
        assert!(to_markdown("Nota", &valid, &FrontMatter::default()).contains("*[Schema]*"));
        assert_eq!(to_plain_text(&valid), "📎 Schema");
    }

    #[test]
    fn image_annotations_are_validated_and_their_notes_are_text() {
        use crate::domain::note::validate_document;
        let image = |annotations: Value| {
            json!({"type": "doc", "content": [{"type": "image", "attrs": {
                "attachmentId": "abc", "alt": "Lavagna", "width": 400, "annotations": annotations
            }}]})
        };
        let valid = image(json!([
            {"kind": "pen", "color": "#e53935", "size": 0.006, "points": [[0.1, 0.1], [0.2, 0.25], [0.3, 0.2]]},
            {"kind": "highlighter", "color": "#fdd835", "size": 0.03, "points": [[0.1, 0.5], [0.6, 0.5]]},
            {"kind": "arrow", "color": "#1e88e5", "size": 0.005, "points": [[0.8, 0.8], [0.6, 0.6]]},
            {"kind": "rect", "color": "#43a047", "size": 0.004, "points": [[0.2, 0.2], [0.4, 0.4]]},
            {"kind": "text", "color": "#111111", "size": 0.04, "points": [[0.5, 0.1]], "text": "Scadenza  venerdì"}
        ]));
        validate_document(&valid).expect("valid annotations");
        assert_eq!(to_plain_text(&valid), "📎 Lavagna — Scadenza venerdì");
        assert!(to_markdown("N", &valid, &FrontMatter::default()).contains("*Scadenza venerdì*"));
        for invalid in [
            json!([{"kind": "pen", "color": "red", "size": 0.01, "points": [[0, 0], [1, 1]]}]),
            json!([{"kind": "pen", "color": "#ff0000", "size": 0.01, "points": [[0, 0]]}]),
            json!([{"kind": "rect", "color": "#ff0000", "size": 0.01, "points": [[0, 0], [3, 1]]}]),
            json!([{"kind": "text", "color": "#ff0000", "size": 0.01, "points": [[0, 0]]}]),
            json!([{"kind": "pen", "color": "#ff0000", "size": 0.01, "points": [[0, 0], [1, 1]], "href": "x"}]),
            json!([{"kind": "script", "color": "#ff0000", "size": 0.01, "points": [[0, 0]]}]),
        ] {
            assert!(
                validate_document(&image(invalid.clone())).is_err(),
                "{invalid}"
            );
        }
    }

    use serde_json::json;

    use super::*;
    use crate::domain::note::validate_document;

    fn paragraph(text: &str) -> Value {
        json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
    }

    #[test]
    fn checklists_tables_and_styles_are_exported() {
        let document = json!({ "type": "doc", "content": [
            { "type": "taskList", "content": [
                { "type": "taskItem", "attrs": { "checked": true }, "content": [paragraph("Fatto")] },
                { "type": "taskItem", "attrs": { "checked": false }, "content": [paragraph("Da fare")] }
            ]},
            { "type": "table", "content": [
                { "type": "tableRow", "content": [
                    { "type": "tableHeader", "content": [paragraph("Nome")] },
                    { "type": "tableHeader", "content": [paragraph("Valore")] }
                ]},
                { "type": "tableRow", "content": [
                    { "type": "tableCell", "content": [paragraph("a|b")] },
                    { "type": "tableCell", "content": [paragraph("1")] }
                ]}
            ]},
            { "type": "paragraph", "content": [
                { "type": "text", "text": "evidenziato", "marks": [
                    { "type": "highlight", "attrs": { "color": "#ffe066" } },
                    { "type": "textStyle", "attrs": { "color": "#aa0000", "fontFamily": "serif", "fontSize": "20px" } }
                ]}
            ]}
        ]});
        validate_document(&document).expect("valid document");

        let markdown = to_markdown("Prova", &document, &FrontMatter::default());
        assert!(
            markdown.contains("- [x] Fatto\n- [ ] Da fare"),
            "{markdown}"
        );
        assert!(
            markdown.contains("| Nome | Valore |\n| --- | --- |\n| a\\|b | 1 |"),
            "{markdown}"
        );
        assert!(markdown.contains("evidenziato"));

        let text = to_plain_text(&document);
        assert!(
            text.contains("[x] Fatto") && text.contains("Nome | Valore"),
            "{text}"
        );

        let html = to_html("Prova", &document, "it");
        assert!(
            html.contains("<input type=\"checkbox\" disabled checked> <p>Fatto</p>"),
            "{html}"
        );
        assert!(html.contains("<th><p>Nome</p></th>") && html.contains("<td><p>1</p></td>"));
        assert!(html.contains("<mark style=\"background:#ffe066\">"));
        assert!(html.contains("<span style=\"color:#aa0000;font-family:serif;font-size:20px\">"));
    }

    #[test]
    fn unsafe_text_styles_are_rejected() {
        for attrs in [
            json!({ "color": "red; background:url(https://example.com/x)" }),
            json!({ "fontFamily": "Comic Sans\"><script>" }),
            json!({ "fontSize": "999px" }),
            json!({ "position": "fixed" }),
        ] {
            let document = json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [
                { "type": "text", "text": "x", "marks": [{ "type": "textStyle", "attrs": attrs }] }
            ]}]});
            assert!(validate_document(&document).is_err(), "{document}");
        }
        let highlight = json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [
            { "type": "text", "text": "x", "marks": [{ "type": "highlight", "attrs": { "color": "expression(alert(1))" } }] }
        ]}]});
        assert!(validate_document(&highlight).is_err());
        let defaults = json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [
            { "type": "text", "text": "x", "marks": [{ "type": "textStyle", "attrs": { "color": null, "fontSize": null } }] }
        ]}]});
        validate_document(&defaults).expect("null attributes are allowed");
    }

    fn sample_document() -> Value {
        json!({
            "type": "doc",
            "content": [
                { "type": "heading", "attrs": { "level": 2 }, "content": [{ "type": "text", "text": "Obiettivi" }] },
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "Testo " },
                    { "type": "text", "text": "grassetto", "marks": [{ "type": "bold" }] },
                    { "type": "text", "text": ", " },
                    { "type": "text", "text": "corsivo", "marks": [{ "type": "italic" }] },
                    { "type": "text", "text": " e " },
                    { "type": "text", "text": "sito", "marks": [{ "type": "link", "attrs": { "href": "https://example.org/a_b" } }] },
                    { "type": "text", "text": " con " },
                    { "type": "text", "text": "codice", "marks": [{ "type": "code" }] },
                    { "type": "text", "text": ", " },
                    { "type": "text", "text": "barrato", "marks": [{ "type": "strike" }] },
                    { "type": "text", "text": " e " },
                    { "type": "text", "text": "sottolineato", "marks": [{ "type": "underline" }] },
                    { "type": "text", "text": ". Simboli: *stelle* [quadre] 2 < 3 & #1" }
                ]},
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "uno" }] }] },
                    { "type": "listItem", "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "due" }] },
                        { "type": "orderedList", "attrs": { "start": 3 }, "content": [
                            { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "annidato" }] }] }
                        ]}
                    ]}
                ]},
                { "type": "blockquote", "content": [
                    { "type": "paragraph", "content": [
                        { "type": "text", "text": "citazione" },
                        { "type": "hardBreak" },
                        { "type": "text", "text": "- seconda riga" }
                    ]}
                ]},
                { "type": "codeBlock", "attrs": { "language": "rust" }, "content": [{ "type": "text", "text": "fn main() {\n    println!(\"```\");\n}" }] },
                { "type": "horizontalRule" },
                { "type": "paragraph", "content": [{ "type": "text", "text": "1. non è una lista" }] }
            ]
        })
    }

    #[test]
    fn markdown_round_trip_preserves_structure_marks_and_metadata() {
        let document = sample_document();
        let front_matter = FrontMatter {
            created_at: Some("2026-01-02T03:04:05.678Z".to_owned()),
            updated_at: Some("2026-02-03T04:05:06Z".to_owned()),
            tags: vec!["progetto".to_owned(), "con spazio".to_owned()],
            notebook: Some("Lavoro".to_owned()),
            ..FrontMatter::default()
        };
        let markdown = to_markdown("Piano: \"annuale\"", &document, &front_matter);
        let parsed = markdown_to_document(&markdown);
        assert_eq!(parsed.title.as_deref(), Some("Piano: \"annuale\""));
        assert_eq!(parsed.document, document, "markdown:\n{markdown}");
        assert_eq!(parsed.front_matter.tags, front_matter.tags);
        assert_eq!(parsed.front_matter.created_at, front_matter.created_at);
        assert_eq!(parsed.front_matter.updated_at, front_matter.updated_at);
        assert_eq!(parsed.front_matter.notebook.as_deref(), Some("Lavoro"));
        validate_document(&parsed.document).expect("imported document is valid");
    }

    #[test]
    fn markdown_import_uses_leading_heading_as_title_and_handles_tight_lists() {
        let parsed = markdown_to_document(
            "# Spesa\n\n- latte\n- [x] pane\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
        );
        assert_eq!(parsed.title.as_deref(), Some("Spesa"));
        validate_document(&parsed.document).expect("valid document");
        let text = to_plain_text(&parsed.document);
        assert!(text.contains("- latte"), "{text}");
        assert!(text.contains("☑ pane"), "{text}");
        assert!(text.contains("1 | 2"), "{text}");
    }

    #[test]
    fn unsafe_links_and_html_are_dropped_on_import() {
        let parsed = markdown_to_document(
            "[clic](javascript:alert(1)) [file](file:///etc/passwd)\n\n<script>alert(1)</script>\n\n<div onclick=\"x()\">blocco <b>sicuro</b></div>\n",
        );
        let serialized = parsed.document.to_string();
        assert!(!serialized.contains("javascript"));
        assert!(!serialized.contains("file:"));
        assert!(!serialized.contains("alert"));
        assert!(!serialized.contains("onclick"));
        assert!(serialized.contains("sicuro"));
        validate_document(&parsed.document).expect("valid document");
    }

    #[test]
    fn html_import_keeps_allowed_structure_only() {
        let parsed = html_to_document(
            r#"<!doctype html><html><head><title>Pagina</title><style>p{}</style>
            <script>steal()</script></head><body>
            <h2>Sezione</h2><p onmouseover="x()">Ciao <strong>mondo</strong>
            <a href="https://example.org">link</a> <a href="javascript:x()">no</a></p>
            <ul><li>uno<ul><li>due</li></ul></li></ul>
            <pre><code class="language-js">let a = 1;
let b = 2;</code></pre>
            <iframe src="https://evil.example"></iframe>
            <img src="x" onerror="x()" alt="figura">
            <table><tr><td>a</td><td>b</td></tr></table></body></html>"#,
        );
        assert_eq!(parsed.title.as_deref(), Some("Pagina"));
        validate_document(&parsed.document).expect("valid document");
        let serialized = parsed.document.to_string();
        for forbidden in [
            "steal",
            "onmouseover",
            "javascript",
            "evil",
            "onerror",
            "p{}",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "{forbidden} in {serialized}"
            );
        }
        let text = to_plain_text(&parsed.document);
        assert!(text.contains("Ciao mondo link no"), "{text}");
        assert!(text.contains("let a = 1;\nlet b = 2;"), "{text}");
        assert!(text.contains("a | b"), "{text}");
        assert!(serialized.contains("\"language\":\"js\""));
        assert!(serialized.contains("https://example.org"));
    }

    #[test]
    fn html_export_escapes_text_and_unsafe_links() {
        let document = json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [
            { "type": "text", "text": "<script>alert('x')</script>" },
            { "type": "text", "text": "evil", "marks": [{ "type": "link", "attrs": { "href": "javascript:alert(1)" } }] }
        ]}]});
        let html = to_html("<b>Titolo</b>", &document, "it");
        assert!(!html.contains("<script>"));
        assert!(!html.contains("javascript:"));
        assert!(html.contains("&lt;b&gt;Titolo&lt;/b&gt;"));
        assert!(html.contains("default-src 'none'"));
        let reparsed = html_to_document(&html);
        assert_eq!(reparsed.title.as_deref(), Some("<b>Titolo</b>"));
        assert!(to_plain_text(&reparsed.document).contains("<script>alert('x')</script>evil"));
    }

    #[test]
    fn html_round_trip_preserves_text_structure() {
        let document = sample_document();
        let reparsed = html_to_document(&to_html("Titolo", &document, "it"));
        validate_document(&reparsed.document).expect("valid document");
        assert_eq!(to_plain_text(&reparsed.document), to_plain_text(&document));
    }

    #[test]
    fn plain_text_conversion_round_trips_paragraphs_and_lines() {
        let document = text_to_document("Prima riga\nseconda riga\r\n\r\nNuovo paragrafo\n");
        validate_document(&document).expect("valid document");
        assert_eq!(
            to_plain_text(&document),
            "Prima riga\nseconda riga\n\nNuovo paragrafo"
        );
        assert_eq!(to_plain_text(&text_to_document("")), "");
    }

    #[test]
    fn deep_nesting_is_flattened_within_validation_limits() {
        let markdown = (0..40)
            .map(|level| format!("{}- livello {level}\n", "  ".repeat(level)))
            .collect::<String>();
        let parsed = markdown_to_document(&markdown);
        validate_document(&parsed.document).expect("deep markdown stays valid");
        let html = format!(
            "{}testo{}",
            "<blockquote>".repeat(300),
            "</blockquote>".repeat(300)
        );
        let parsed = html_to_document(&html);
        validate_document(&parsed.document).expect("deep html stays valid");
        assert!(to_plain_text(&parsed.document).contains("testo"));
    }

    #[test]
    fn timestamps_are_validated() {
        assert!(is_utc_timestamp("2026-10-14T08:00:00Z"));
        assert!(is_utc_timestamp("2026-10-14T08:00:00.123Z"));
        assert!(!is_utc_timestamp("2026-10-14 08:00:00"));
        assert!(!is_utc_timestamp("2026-10-14T08:00:00.Z"));
    }

    #[test]
    fn encrypted_text_is_validated_hidden_and_round_trips_through_exports() {
        use crate::{domain::note::validate_document, secret_text};
        let armored = secret_text::encrypt("PIN 4321", "password lunga").expect("encrypt");
        let document = |attrs: Value| {
            json!({"type": "doc", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Conto"}]},
                {"type": "encryptedText", "attrs": attrs},
            ]})
        };
        let valid = document(json!({"ciphertext": armored, "hint": "il solito"}));
        validate_document(&valid).expect("valid encrypted text");
        for invalid in [
            json!({"hint": "senza testo cifrato"}),
            json!({"ciphertext": "PIN 4321"}),
            json!({"ciphertext": armored, "hint": "a\nb"}),
            json!({"ciphertext": armored, "plaintext": "PIN 4321"}),
        ] {
            assert!(
                validate_document(&document(invalid.clone())).is_err(),
                "{invalid}"
            );
        }
        let with_content = json!({"type": "doc", "content": [{"type": "encryptedText",
            "attrs": {"ciphertext": armored}, "content": [{"type": "text", "text": "PIN"}]}]});
        assert!(validate_document(&with_content).is_err());

        // Ricerca, AI e anteprime vedono solo il segnaposto.
        assert_eq!(to_plain_text(&valid), "Conto\n\n🔒");
        // Il testo semplice esportato conserva il testo cifrato.
        assert!(to_plain_text(&encrypted_text_as_code(&valid)).contains(&armored));

        let markdown = to_markdown("Banca", &valid, &FrontMatter::default());
        assert!(markdown.contains(&format!("```age\n{armored}\n```")));
        assert!(!markdown.contains("4321"));
        let imported = markdown_to_document(&markdown).document;
        validate_document(&imported).expect("valid import");
        assert_eq!(imported["content"][1]["type"], "encryptedText");
        assert_eq!(imported["content"][1]["attrs"]["ciphertext"], armored);

        let html = to_html("Banca", &valid, "it");
        assert!(!html.contains("4321"));
        let imported = html_to_document(&html).document;
        validate_document(&imported).expect("valid import");
        let found = imported["content"]
            .as_array()
            .expect("content")
            .iter()
            .any(|node| node["type"] == "encryptedText" && node["attrs"]["ciphertext"] == armored);
        assert!(found, "{imported}");
        assert_eq!(
            secret_text::decrypt(&armored, "password lunga").expect("decrypt"),
            "PIN 4321"
        );

        // Un blocco `age` che non contiene un testo cifrato resta codice.
        let plain = markdown_to_document("```age\nnon cifrato\n```").document;
        assert_eq!(plain["content"][0]["type"], "codeBlock");
    }
}
