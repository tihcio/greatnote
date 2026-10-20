use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub notebook_id: Option<String>,
    pub title: String,
    pub content: Value,
    pub created_at: String,
    pub updated_at: String,
    pub is_pinned: bool,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteDraft {
    pub note_id: String,
    pub title: String,
    pub content: Value,
    pub saved_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub note_id: String,
    pub original_filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub created_at: String,
    /// Lettura del testo dell'immagine: "done", "failed" o assente.
    #[serde(default)]
    pub ocr_status: Option<String>,
    #[serde(default)]
    pub ocr_text: Option<String>,
    /// Motore usato (es. "tesseract", "Claude Code · sonnet").
    #[serde(default)]
    pub ocr_engine: Option<String>,
    #[serde(default)]
    pub ocr_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteLink {
    pub target_note_id: String,
    pub title: String,
    pub label: Option<String>,
}

pub fn empty_document() -> Value {
    serde_json::json!({
        "type": "doc",
        "content": [{ "type": "paragraph" }]
    })
}

pub fn validate_document(document: &Value) -> Result<(), &'static str> {
    let serialized = serde_json::to_vec(document).map_err(|_| "Documento non valido")?;
    if serialized.len() > 1_048_576 {
        return Err("Il contenuto della nota supera il limite di 1 MiB");
    }

    let root = document
        .as_object()
        .ok_or("Il documento deve essere un oggetto JSON")?;
    if root.get("type").and_then(Value::as_str) != Some("doc") {
        return Err("Il documento deve avere il tipo 'doc'");
    }
    let content = root
        .get("content")
        .and_then(Value::as_array)
        .ok_or("Il documento deve contenere un array 'content'")?;

    for node in content {
        validate_node(node, 0)?;
    }

    Ok(())
}

/// Famiglie di caratteri ammesse per `textStyle` (diventano CSS).
pub const FONT_FAMILIES: [&str; 3] = ["sans-serif", "serif", "monospace"];
/// Dimensioni ammesse per `textStyle`.
pub const FONT_SIZES: [&str; 9] = [
    "11px", "13px", "15px", "17px", "20px", "24px", "28px", "32px", "40px",
];

/// Colore CSS esadecimale `#rgb` o `#rrggbb`: nessun altro valore può finire
/// negli stili generati dall'editor o dall'esportazione HTML.
pub fn is_hex_color(value: &str) -> bool {
    value
        .strip_prefix('#')
        .is_some_and(|hex| matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

fn validate_style_attrs(mark: &Value) -> Result<(), &'static str> {
    let Some(attrs) = mark.get("attrs") else {
        return Ok(());
    };
    let attrs = attrs
        .as_object()
        .ok_or("Gli attributi di un contrassegno devono essere un oggetto")?;
    let mark_type = mark.get("type").and_then(Value::as_str).unwrap_or("");
    for (key, value) in attrs {
        let valid = match (mark_type, key.as_str()) {
            (_, _) if value.is_null() => true,
            ("highlight" | "textStyle", "color") => value.as_str().is_some_and(is_hex_color),
            ("textStyle", "fontFamily") => value
                .as_str()
                .is_some_and(|family| FONT_FAMILIES.contains(&family)),
            ("textStyle", "fontSize") => value
                .as_str()
                .is_some_and(|size| FONT_SIZES.contains(&size)),
            ("highlight" | "textStyle", _) => false,
            _ => true,
        };
        if !valid {
            return Err("Il documento contiene uno stile del testo non valido");
        }
    }
    Ok(())
}

/// Immagine nel testo: solo un riferimento a un allegato della nota (mai un
/// URL o un percorso), testo alternativo e larghezza facoltativi.
fn validate_image(object: &serde_json::Map<String, Value>) -> Result<(), &'static str> {
    const INVALID: &str = "Il documento contiene un'immagine non valida";
    if object.contains_key("content") || object.contains_key("marks") {
        return Err(INVALID);
    }
    let attrs = object
        .get("attrs")
        .and_then(Value::as_object)
        .ok_or(INVALID)?;
    for (key, value) in attrs {
        let valid = match key.as_str() {
            "attachmentId" => value.as_str().is_some_and(is_attachment_id),
            "alt" => {
                value.is_null() || value.as_str().is_some_and(|alt| alt.chars().count() <= 500)
            }
            "width" => {
                value.is_null()
                    || value
                        .as_u64()
                        .is_some_and(|width| (40..=2400).contains(&width))
            }
            "annotations" => value.is_null() || validate_annotations(value).is_ok(),
            _ => false,
        };
        if !valid {
            return Err(INVALID);
        }
    }
    if !attrs.contains_key("attachmentId") {
        return Err(INVALID);
    }
    Ok(())
}

/// Annotazioni massime per immagine e punti per tratto.
pub const MAX_IMAGE_ANNOTATIONS: usize = 300;
pub const MAX_ANNOTATION_POINTS: usize = 2000;

/// Annotazioni disegnate su un'immagine: forme vettoriali con coordinate
/// relative all'immagine (0–1, con un piccolo margine), colore esadecimale e
/// spessore relativo alla larghezza. Nessun HTML né URL.
///
/// - `pen`, `highlighter`: tratto libero (2+ punti);
/// - `arrow`, `rect`, `ellipse`: due punti (inizio e fine);
/// - `text`: un punto e un testo (nota scritta sull'immagine).
pub fn validate_annotations(value: &Value) -> Result<(), &'static str> {
    const INVALID: &str = "Le annotazioni dell'immagine non sono valide";
    let shapes = value.as_array().ok_or(INVALID)?;
    if shapes.len() > MAX_IMAGE_ANNOTATIONS {
        return Err("Troppe annotazioni sull'immagine");
    }
    let coordinate = |value: &Value| value.as_f64().is_some_and(|v| (-0.05..=1.05).contains(&v));
    for shape in shapes {
        let shape = shape.as_object().ok_or(INVALID)?;
        if shape
            .keys()
            .any(|key| !matches!(key.as_str(), "kind" | "color" | "size" | "points" | "text"))
        {
            return Err(INVALID);
        }
        let kind = shape.get("kind").and_then(Value::as_str).ok_or(INVALID)?;
        let color_ok = shape
            .get("color")
            .and_then(Value::as_str)
            .is_some_and(|color| color.len() == 7 && is_hex_color(color));
        let size_ok = shape
            .get("size")
            .and_then(Value::as_f64)
            .is_some_and(|size| (0.0005..=0.2).contains(&size));
        let points = shape
            .get("points")
            .and_then(Value::as_array)
            .ok_or(INVALID)?;
        let points_ok = points.iter().all(|point| {
            point
                .as_array()
                .is_some_and(|pair| pair.len() == 2 && pair.iter().all(coordinate))
        });
        let count_ok = match kind {
            "pen" | "highlighter" => (2..=MAX_ANNOTATION_POINTS).contains(&points.len()),
            "arrow" | "rect" | "ellipse" => points.len() == 2,
            "text" => points.len() == 1,
            _ => false,
        };
        let text_ok = match (kind, shape.get("text")) {
            ("text", Some(text)) => text
                .as_str()
                .is_some_and(|text| !text.trim().is_empty() && text.chars().count() <= 500),
            ("text", None) => false,
            (_, None) => true,
            (_, Some(_)) => false,
        };
        if !(color_ok && size_ok && points_ok && count_ok && text_ok) {
            return Err(INVALID);
        }
    }
    Ok(())
}

/// Identificativo di allegato: caratteri alfanumerici e trattini.
pub fn is_attachment_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Sostituisce gli id degli allegati nelle immagini (copie di note,
/// importazioni). Gli id non presenti nella mappa restano invariati.
pub fn remap_image_attachments(
    document: &mut Value,
    map: &std::collections::HashMap<String, String>,
) {
    match document {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("image")
                && let Some(attrs) = object.get_mut("attrs").and_then(Value::as_object_mut)
                && let Some(new_id) = attrs
                    .get("attachmentId")
                    .and_then(Value::as_str)
                    .and_then(|id| map.get(id))
            {
                attrs.insert("attachmentId".to_owned(), Value::String(new_id.clone()));
            }
            if let Some(content) = object.get_mut("content") {
                remap_image_attachments(content, map);
            }
        }
        Value::Array(items) => {
            for item in items {
                remap_image_attachments(item, map);
            }
        }
        _ => {}
    }
}

fn validate_node(node: &Value, depth: usize) -> Result<(), &'static str> {
    if depth > 20 {
        return Err("La struttura del documento è troppo profonda");
    }

    let object = node
        .as_object()
        .ok_or("Ogni nodo del documento deve essere un oggetto")?;
    let node_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("Ogni nodo del documento deve avere un tipo")?;
    if !matches!(
        node_type,
        "paragraph"
            | "heading"
            | "bulletList"
            | "orderedList"
            | "listItem"
            | "blockquote"
            | "codeBlock"
            | "horizontalRule"
            | "hardBreak"
            | "text"
            | "taskList"
            | "taskItem"
            | "table"
            | "tableRow"
            | "tableHeader"
            | "tableCell"
            | "image"
    ) {
        return Err("Il documento contiene un tipo di nodo non supportato");
    }
    if node_type == "image" {
        validate_image(object)?;
    }

    if node_type == "text" && object.get("text").and_then(Value::as_str).is_none() {
        return Err("Un nodo di testo deve contenere una stringa");
    }
    if let Some(marks) = object.get("marks") {
        let marks = marks
            .as_array()
            .ok_or("I contrassegni del testo devono essere un array")?;
        for mark in marks {
            let mark_type = mark
                .get("type")
                .and_then(Value::as_str)
                .ok_or("Ogni contrassegno deve avere un tipo")?;
            if !matches!(
                mark_type,
                "bold"
                    | "italic"
                    | "strike"
                    | "underline"
                    | "code"
                    | "link"
                    | "highlight"
                    | "textStyle"
            ) {
                return Err("Il documento contiene un contrassegno non supportato");
            }
            validate_style_attrs(mark)?;
        }
    }
    if let Some(children) = object.get("content") {
        let children = children
            .as_array()
            .ok_or("Il contenuto di un nodo deve essere un array")?;
        for child in children {
            validate_node(child, depth + 1)?;
        }
    }

    Ok(())
}
