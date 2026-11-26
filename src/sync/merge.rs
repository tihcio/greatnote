//! Fusione a tre vie del testo di una nota modificata in contemporanea su
//! due dispositivi (ADR 0034).
//!
//! Il documento dell'editor è un albero di blocchi. Si confrontano i blocchi
//! della versione di partenza comune con quelli delle due versioni; se le
//! modifiche toccano blocchi diversi si uniscono. Se toccano lo stesso
//! blocco con figli (elenco, citazione, tabella…) si scende nei figli: così
//! due voci diverse dello stesso elenco si uniscono. Modifiche allo stesso
//! paragrafo, o inserimenti nello stesso punto, non si uniscono: in quel
//! caso resta la regola di sempre (una versione vince, l'altra va nella
//! cronologia).
//!
//! Il risultato dipende solo dai tre documenti e da quale versione vince,
//! quindi due dispositivi che fondono le stesse versioni ottengono lo
//! stesso testo.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::folder::Version;

/// Oltre questa soglia (blocchi × blocchi) il confronto costerebbe troppo.
const MAX_CELLS: usize = 4_000_000;
/// Profondità massima di discesa nei blocchi con figli.
const MAX_DEPTH: usize = 8;

/// Versioni di ogni nota di cui si conserva il testo: abbastanza per
/// risalire alla partenza comune di modifiche contemporanee recenti.
const KEPT_VERSIONS: i64 = 8;

/// Conserva titolo e testo di una versione di nota, con la sua base.
pub(super) fn remember(
    connection: &Connection,
    note_id: &str,
    version: &Version,
    base: Option<&Version>,
    row: &Value,
) -> Result<(), rusqlite::Error> {
    let (Some(title), Some(content)) = (row["title"].as_str(), row["content_json"].as_str()) else {
        return Ok(());
    };
    connection.execute(
        "INSERT OR REPLACE INTO sync_note_bases
             (note_id, counter, device, base_counter, base_device, title, content_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            note_id,
            version.0,
            version.1,
            base.map(|base| base.0),
            base.map(|base| base.1.as_str()),
            title,
            content
        ],
    )?;
    connection.execute(
        "DELETE FROM sync_note_bases
         WHERE note_id = ?1 AND (counter, device) NOT IN (
             SELECT counter, device FROM sync_note_bases WHERE note_id = ?1
             ORDER BY counter DESC, device DESC LIMIT ?2)",
        params![note_id, KEPT_VERSIONS],
    )?;
    Ok(())
}

/// Titolo e testo conservati di una versione.
fn stored(
    connection: &Connection,
    note_id: &str,
    version: &Version,
) -> Result<Option<(String, Value)>, rusqlite::Error> {
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT title, content_json FROM sync_note_bases
             WHERE note_id = ?1 AND counter = ?2 AND device = ?3",
            params![note_id, version.0, version.1],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(title, content)| {
        serde_json::from_str(&content)
            .ok()
            .map(|content| (title, content))
    }))
}

/// `ancestor` è nella storia di `version` (tra le versioni conservate).
fn descends_from(
    connection: &Connection,
    note_id: &str,
    version: &Version,
    ancestor: &Version,
) -> Result<bool, rusqlite::Error> {
    if version == ancestor {
        return Ok(true);
    }
    let mut current = version.clone();
    for _ in 0..KEPT_VERSIONS {
        let base: Option<(Option<i64>, Option<String>)> = connection
            .query_row(
                "SELECT base_counter, base_device FROM sync_note_bases
                 WHERE note_id = ?1 AND counter = ?2 AND device = ?3",
                params![note_id, current.0, current.1],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match base {
            Some((Some(counter), Some(device))) => {
                current = Version(counter, device);
                if current == *ancestor {
                    return Ok(true);
                }
            }
            _ => return Ok(false),
        }
    }
    Ok(false)
}

/// Esito della fusione di due versioni contemporanee di una nota.
pub(super) enum Merged {
    /// Le modifiche della versione perdente sono già nella vincente.
    Contained,
    /// Testo fuso diverso da entrambe.
    Changed { title: String, content: Value },
}

/// Prova a fondere la versione locale e quella remota di una nota. Serve la
/// versione di partenza della modifica remota, conservata e antenata di
/// quella locale; altrimenti `None` (vale la regola del vincitore).
pub(super) fn try_merge(
    connection: &Connection,
    note_id: &str,
    local_version: &Version,
    local: &Value,
    remote_base: &Version,
    remote: &Value,
    remote_wins: bool,
) -> Result<Option<Merged>, rusqlite::Error> {
    if !descends_from(connection, note_id, local_version, remote_base)? {
        return Ok(None);
    }
    let Some((base_title, base_content)) = stored(connection, note_id, remote_base)? else {
        return Ok(None);
    };
    let parse = |row: &Value| -> Option<(String, Value)> {
        let title = row["title"].as_str()?.to_owned();
        let content = serde_json::from_str(row["content_json"].as_str()?).ok()?;
        Some((title, content))
    };
    let (Some(local), Some(remote)) = (parse(local), parse(remote)) else {
        return Ok(None);
    };
    let (winner, loser) = if remote_wins {
        (&remote, &local)
    } else {
        (&local, &remote)
    };
    let Some((title, content)) = merge_note(
        (&base_title, &base_content),
        (&winner.0, &winner.1),
        (&loser.0, &loser.1),
    ) else {
        return Ok(None);
    };
    if crate::domain::note::validate_document(&content).is_err() {
        return Ok(None);
    }
    Ok(Some(if title == winner.0 && content == winner.1 {
        Merged::Contained
    } else {
        Merged::Changed { title, content }
    }))
}

/// Titolo e testo fusi, oppure `None` se le modifiche si sovrappongono.
/// `winner` è la versione che vincerebbe senza fusione: decide solo a parità
/// (attributi cambiati allo stesso modo).
pub(super) fn merge_note(
    base: (&str, &Value),
    winner: (&str, &Value),
    loser: (&str, &Value),
) -> Option<(String, Value)> {
    let title = merge_field(base.0, winner.0, loser.0)?.to_owned();
    let content = merge_node(base.1, winner.1, loser.1, 0)?;
    Some((title, content))
}

/// Campo semplice: cambiato da una parte sola, o allo stesso modo.
fn merge_field<'a, T: PartialEq + ?Sized>(
    base: &'a T,
    winner: &'a T,
    loser: &'a T,
) -> Option<&'a T> {
    if winner == loser || loser == base {
        Some(winner)
    } else if winner == base {
        Some(loser)
    } else {
        None
    }
}

/// Figli di un nodo (nessuno se manca `content`).
fn children(node: &serde_json::Map<String, Value>) -> Option<&[Value]> {
    match node.get("content") {
        None => Some(&[]),
        Some(Value::Array(items)) => Some(items),
        Some(_) => None,
    }
}

/// Un nodo con le stesse chiavi tranne `content`, i cui figli vengono fusi.
fn merge_node(base: &Value, winner: &Value, loser: &Value, depth: usize) -> Option<Value> {
    if let Some(value) = merge_field(base, winner, loser) {
        return Some(value.clone());
    }
    if depth >= MAX_DEPTH {
        return None;
    }
    let (Some(b), Some(w), Some(l)) = (base.as_object(), winner.as_object(), loser.as_object())
    else {
        return None;
    };
    // Tipo uguale; attributi e marchi cambiati da una parte sola.
    let mut merged = serde_json::Map::new();
    let keys: std::collections::BTreeSet<&String> =
        b.keys().chain(w.keys()).chain(l.keys()).collect();
    for key in keys {
        if key == "content" {
            continue;
        }
        let (bv, wv, lv) = (b.get(key), w.get(key), l.get(key));
        if let Some(value) = *merge_field(&bv, &wv, &lv)? {
            merged.insert(key.clone(), value.clone());
        }
    }
    let content = merge_children(children(b)?, children(w)?, children(l)?, depth + 1)?;
    if !content.is_empty() || w.contains_key("content") {
        merged.insert("content".to_owned(), Value::Array(content));
    }
    Some(Value::Object(merged))
}

/// Tratto della versione di partenza `[start, end)` sostituito da `nodes`.
#[derive(Debug, Clone, PartialEq)]
struct Hunk {
    start: usize,
    end: usize,
    nodes: Vec<Value>,
}

/// Differenze tra due elenchi di nodi (sottosequenza comune più lunga).
fn hunks(base: &[Value], other: &[Value]) -> Option<Vec<Hunk>> {
    let mut prefix = 0;
    while prefix < base.len() && prefix < other.len() && base[prefix] == other[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < base.len() - prefix
        && suffix < other.len() - prefix
        && base[base.len() - 1 - suffix] == other[other.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let a = &base[prefix..base.len() - suffix];
    let b = &other[prefix..other.len() - suffix];
    if a.is_empty() && b.is_empty() {
        return Some(Vec::new());
    }
    if (a.len() + 1).saturating_mul(b.len() + 1) > MAX_CELLS {
        return None;
    }
    let cols = b.len() + 1;
    let mut table = vec![0u32; (a.len() + 1) * cols];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            table[i * cols + j] = if a[i] == b[j] {
                table[(i + 1) * cols + j + 1] + 1
            } else {
                table[(i + 1) * cols + j].max(table[i * cols + j + 1])
            };
        }
    }
    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut open: Option<Hunk> = None;
    let close = |open: &mut Option<Hunk>, result: &mut Vec<Hunk>| {
        if let Some(hunk) = open.take() {
            result.push(hunk);
        }
    };
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            close(&mut open, &mut result);
            i += 1;
            j += 1;
            continue;
        }
        let hunk = open.get_or_insert_with(|| Hunk {
            start: prefix + i,
            end: prefix + i,
            nodes: Vec::new(),
        });
        if j < b.len() && (i == a.len() || table[i * cols + j + 1] >= table[(i + 1) * cols + j]) {
            hunk.nodes.push(b[j].clone());
            j += 1;
        } else {
            i += 1;
            hunk.end = prefix + i;
        }
    }
    close(&mut open, &mut result);
    Some(result)
}

/// Due modifiche (una per parte) che non si possono applicare entrambe
/// senza ambiguità.
fn clash(a: &Hunk, b: &Hunk) -> bool {
    let overlap = a.start < b.end && b.start < a.end;
    let insertion_touches = (a.start == a.end && b.start <= a.start && a.start <= b.end)
        || (b.start == b.end && a.start <= b.start && b.start <= a.end);
    overlap || insertion_touches
}

fn merge_children(
    base: &[Value],
    winner: &[Value],
    loser: &[Value],
    depth: usize,
) -> Option<Vec<Value>> {
    if let Some(value) = merge_field(base, winner, loser) {
        return Some(value.to_vec());
    }
    let ours = hunks(base, winner)?;
    let theirs = hunks(base, loser)?;
    let mut output = Vec::new();
    let mut position = 0;
    let (mut i, mut j) = (0, 0);
    while i < ours.len() || j < theirs.len() {
        let (next_ours, next_theirs) = (ours.get(i), theirs.get(j));
        let hunk = match (next_ours, next_theirs) {
            (Some(a), Some(b)) if clash(a, b) => {
                i += 1;
                j += 1;
                resolve(base, a, b, depth)?
            }
            (Some(a), Some(b)) => {
                if (a.start, a.end) <= (b.start, b.end) {
                    i += 1;
                    a.clone()
                } else {
                    j += 1;
                    b.clone()
                }
            }
            (Some(a), None) => {
                i += 1;
                a.clone()
            }
            (None, Some(b)) => {
                j += 1;
                b.clone()
            }
            (None, None) => break,
        };
        // Una modifica che si scontra con la successiva dell'altra parte
        // (catena) non si unisce.
        if ours
            .get(i)
            .is_some_and(|next| clash(next, &hunk) && next.start < hunk.end)
            || theirs
                .get(j)
                .is_some_and(|next| clash(next, &hunk) && next.start < hunk.end)
        {
            return None;
        }
        if hunk.start < position {
            return None;
        }
        output.extend_from_slice(&base[position..hunk.start]);
        output.extend(hunk.nodes);
        position = hunk.end;
    }
    output.extend_from_slice(&base[position..]);
    Some(output)
}

/// Due modifiche nello stesso punto: uguali, oppure lo stesso blocco
/// cambiato in punti diversi dei suoi figli.
fn resolve(base: &[Value], a: &Hunk, b: &Hunk, depth: usize) -> Option<Hunk> {
    if a == b {
        return Some(a.clone());
    }
    if a.start == b.start
        && a.end == b.end
        && a.end == a.start + 1
        && a.nodes.len() == 1
        && b.nodes.len() == 1
        && a.nodes[0].get("type") == base[a.start].get("type")
        && b.nodes[0].get("type") == base[a.start].get("type")
    {
        let node = merge_node(&base[a.start], &a.nodes[0], &b.nodes[0], depth)?;
        return Some(Hunk {
            start: a.start,
            end: a.end,
            nodes: vec![node],
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn p(text: &str) -> Value {
        json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
    }

    fn doc(blocks: Vec<Value>) -> Value {
        json!({ "type": "doc", "content": blocks })
    }

    fn list(items: &[&str]) -> Value {
        json!({ "type": "bulletList", "content": items.iter().map(|item| json!({
            "type": "listItem", "content": [p(item)]
        })).collect::<Vec<_>>() })
    }

    fn merge(base: &Value, winner: &Value, loser: &Value) -> Option<Value> {
        merge_note(("T", base), ("T", winner), ("T", loser)).map(|(_, content)| content)
    }

    #[test]
    fn edits_to_different_paragraphs_are_merged_symmetrically() {
        let base = doc(vec![p("uno"), p("due"), p("tre")]);
        let a = doc(vec![p("uno modificato"), p("due"), p("tre")]);
        let b = doc(vec![p("uno"), p("due"), p("tre"), p("quattro")]);
        let expected = doc(vec![p("uno modificato"), p("due"), p("tre"), p("quattro")]);
        assert_eq!(merge(&base, &a, &b), Some(expected.clone()));
        assert_eq!(merge(&base, &b, &a), Some(expected));
        // Paragrafi vicini ma distinti.
        let c = doc(vec![p("uno"), p("due cambiato"), p("tre")]);
        let d = doc(vec![p("uno"), p("due"), p("tre cambiato")]);
        assert_eq!(
            merge(&base, &c, &d),
            Some(doc(vec![p("uno"), p("due cambiato"), p("tre cambiato")]))
        );
        // Una parte elimina un paragrafo, l'altra ne modifica un altro.
        let e = doc(vec![p("uno"), p("tre")]);
        assert_eq!(
            merge(&base, &e, &a),
            Some(doc(vec![p("uno modificato"), p("tre")]))
        );
    }

    #[test]
    fn overlapping_edits_are_not_merged() {
        let base = doc(vec![p("uno"), p("due")]);
        let a = doc(vec![p("uno A"), p("due")]);
        let b = doc(vec![p("uno B"), p("due")]);
        assert_eq!(merge(&base, &a, &b), None);
        // Inserimenti diversi nello stesso punto: ordine ambiguo.
        let c = doc(vec![p("uno"), p("C"), p("due")]);
        let d = doc(vec![p("uno"), p("D"), p("due")]);
        assert_eq!(merge(&base, &c, &d), None);
        // Eliminazione di un paragrafo modificato dall'altra parte.
        let e = doc(vec![p("due")]);
        assert_eq!(merge(&base, &a, &e), None);
        // Inserimento accanto a un paragrafo modificato.
        let f = doc(vec![p("uno"), p("nuovo"), p("due")]);
        assert_eq!(merge(&base, &a, &f), None);
        // Titoli cambiati in modo diverso.
        assert_eq!(merge_note(("T", &base), ("A", &a), ("B", &c)), None);
    }

    #[test]
    fn same_change_titles_and_lists() {
        let base = doc(vec![p("uno"), list(&["pane", "latte", "uova"])]);
        // La stessa modifica fatta da entrambi.
        let same = doc(vec![p("uno!"), list(&["pane", "latte", "uova"])]);
        assert_eq!(merge(&base, &same, &same), Some(same.clone()));
        // Voci diverse dello stesso elenco.
        let a = doc(vec![p("uno"), list(&["pane integrale", "latte", "uova"])]);
        let b = doc(vec![p("uno"), list(&["pane", "latte", "uova", "caffè"])]);
        assert_eq!(
            merge(&base, &a, &b),
            Some(doc(vec![
                p("uno"),
                list(&["pane integrale", "latte", "uova", "caffè"])
            ]))
        );
        // Titolo cambiato da una parte, testo dall'altra.
        assert_eq!(
            merge_note(("Spesa", &base), ("Spesa", &a), ("Spesa di sabato", &b)).map(|m| m.0),
            Some("Spesa di sabato".to_owned())
        );
    }

    #[test]
    fn attributes_follow_the_side_that_changed_them() {
        let item = |checked: bool, text: &str| json!({ "type": "taskItem", "attrs": { "checked": checked }, "content": [p(text)] });
        let tasks = |items: Vec<Value>| json!({ "type": "taskList", "content": items });
        let base = doc(vec![tasks(vec![
            item(false, "chiamare"),
            item(false, "scrivere"),
        ])]);
        let a = doc(vec![tasks(vec![
            item(true, "chiamare"),
            item(false, "scrivere"),
        ])]);
        let b = doc(vec![tasks(vec![
            item(false, "chiamare"),
            item(false, "scrivere il verbale"),
        ])]);
        assert_eq!(
            merge(&base, &a, &b),
            Some(doc(vec![tasks(vec![
                item(true, "chiamare"),
                item(false, "scrivere il verbale")
            ])]))
        );
        // Stessa voce: spuntata da una parte, testo cambiato dall'altra.
        let c = doc(vec![tasks(vec![
            item(false, "chiamare Anna"),
            item(false, "scrivere"),
        ])]);
        assert_eq!(
            merge(&base, &a, &c),
            Some(doc(vec![tasks(vec![
                item(true, "chiamare Anna"),
                item(false, "scrivere")
            ])]))
        );
    }
}
