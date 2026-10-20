//! Suddivisione del testo di una nota in frammenti per gli embedding e il
//! contesto. Le posizioni sono in caratteri Unicode, così un frammento può
//! sempre essere riletto dalla versione corrente della nota.

/// Lunghezza massima di un frammento.
pub const MAX_CHUNK_CHARS: usize = 1_200;
/// Sotto questa lunghezza non si cerca un punto di taglio migliore.
const MIN_CHUNK_CHARS: usize = 400;
/// Frammenti indicizzati per nota: le note molto lunghe sono indicizzate solo
/// nella parte iniziale.
pub const MAX_CHUNKS_PER_NOTE: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub start: usize,
    pub end: usize,
}

/// Divide il testo preferendo, nell'ordine, fine paragrafo, fine riga, fine
/// frase e spazio. Gli spazi iniziali di ogni frammento sono saltati.
pub fn chunk_text(text: &str) -> Vec<Chunk> {
    let characters: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < characters.len() && chunks.len() < MAX_CHUNKS_PER_NOTE {
        while start < characters.len() && characters[start].is_whitespace() {
            start += 1;
        }
        if start >= characters.len() {
            break;
        }
        let limit = (start + MAX_CHUNK_CHARS).min(characters.len());
        let end = if limit == characters.len() {
            limit
        } else {
            best_break(&characters, start + MIN_CHUNK_CHARS, limit).unwrap_or(limit)
        };
        let mut trimmed_end = end;
        while trimmed_end > start && characters[trimmed_end - 1].is_whitespace() {
            trimmed_end -= 1;
        }
        if trimmed_end > start {
            chunks.push(Chunk {
                start,
                end: trimmed_end,
            });
        }
        start = end;
    }
    chunks
}

fn best_break(characters: &[char], from: usize, limit: usize) -> Option<usize> {
    let window = from..limit;
    let find_last =
        |predicate: &dyn Fn(usize) -> bool| window.clone().rev().find(|index| predicate(*index));
    find_last(&|index| characters[index] == '\n' && characters.get(index + 1) == Some(&'\n'))
        .map(|index| index + 2)
        .or_else(|| find_last(&|index| characters[index] == '\n').map(|index| index + 1))
        .or_else(|| {
            find_last(&|index| {
                matches!(characters[index], '.' | '!' | '?' | ';')
                    && characters
                        .get(index + 1)
                        .is_some_and(|next| next.is_whitespace())
            })
            .map(|index| index + 1)
        })
        .or_else(|| find_last(&|index| characters[index].is_whitespace()).map(|index| index + 1))
}

/// Estrae i caratteri `[start, end)`, limitando gli estremi alla lunghezza.
pub fn char_slice(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_texts_are_a_single_chunk() {
        assert_eq!(
            chunk_text("  Ciao mondo  "),
            vec![Chunk { start: 2, end: 12 }]
        );
        assert!(chunk_text("   \n\n ").is_empty());
    }

    #[test]
    fn long_texts_break_at_paragraphs_and_cover_everything() {
        let paragraph = "Questa è una frase di prova con àccenti. ".repeat(30);
        let text = format!("{paragraph}\n\n{paragraph}\n\n{paragraph}");
        let chunks = chunk_text(&text);
        assert!(chunks.len() >= 3, "{chunks:?}");
        for chunk in &chunks {
            assert!(chunk.end - chunk.start <= MAX_CHUNK_CHARS);
            let slice = char_slice(&text, chunk.start, chunk.end);
            assert_eq!(slice.trim(), slice);
        }
        let joined: String = chunks
            .iter()
            .map(|chunk| char_slice(&text, chunk.start, chunk.end))
            .collect::<Vec<_>>()
            .join(" ");
        let words = |value: &str| value.split_whitespace().count();
        assert_eq!(words(&joined), words(&text));
    }

    #[test]
    fn text_without_spaces_is_cut_at_the_limit() {
        let text = "x".repeat(MAX_CHUNK_CHARS * 2 + 5);
        let chunks = chunk_text(&text);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[2].end, text.len());
    }
}
