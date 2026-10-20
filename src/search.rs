#[derive(Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub terms: Vec<String>,
    /// Per ogni termine: scritto tra virgolette (frase esatta). Le parole
    /// senza virgolette si cercano anche per prefisso ("euron" → "Euronics").
    pub quoted: Vec<bool>,
    pub tags: Vec<String>,
    pub notebooks: Vec<String>,
    pub spaces: Vec<String>,
    pub created_range: Option<(String, String)>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub updated_days: Option<u32>,
    pub has_attachment: bool,
    pub is_pinned: bool,
    pub is_archived: bool,
    /// Note con almeno un'attività aperta collegata (`is:task`, `has:task`).
    pub has_open_task: bool,
}

#[derive(Debug)]
struct Token {
    value: String,
    quoted: bool,
}

pub fn parse_query(input: &str) -> Result<SearchQuery, String> {
    if input.chars().count() > 512 {
        return Err("La ricerca non può superare 512 caratteri".to_owned());
    }

    let tokens = tokenize(input)?;
    if tokens.len() > 32 {
        return Err("La ricerca può contenere al massimo 32 termini o filtri".to_owned());
    }

    let mut query = SearchQuery::default();
    for token in tokens {
        if matches!(token.value.as_str(), "AND" | "OR" | "NOT" | "(" | ")") && !token.quoted {
            return Err(
                "Gli operatori booleani non sono ancora supportati; combina i termini con spazi"
                    .to_owned(),
            );
        }
        if let Some((field, value)) = token.value.split_once(':') {
            let value = value.trim();
            if value.is_empty() {
                return Err(format!("Il filtro {field}: richiede un valore"));
            }
            match field.to_ascii_lowercase().as_str() {
                "tag" => query.tags.push(value.to_owned()),
                "notebook" => query.notebooks.push(value.to_owned()),
                "space" => query.spaces.push(value.to_owned()),
                "created" => {
                    let (start, end) = value
                        .split_once("..")
                        .ok_or_else(|| "Usa created:AAAA-MM-GG..AAAA-MM-GG".to_owned())?;
                    validate_date(start)?;
                    validate_date(end)?;
                    if start > end {
                        return Err("L'intervallo created ha le date invertite".to_owned());
                    }
                    query.created_range = Some((start.to_owned(), end.to_owned()));
                }
                "before" => {
                    validate_date(value)?;
                    query.before = Some(value.to_owned());
                }
                "after" => {
                    validate_date(value)?;
                    query.after = Some(value.to_owned());
                }
                "updated" => {
                    let days = value
                        .strip_suffix('d')
                        .and_then(|value| value.parse::<u32>().ok())
                        .filter(|days| (1..=36_500).contains(days))
                        .ok_or_else(|| "Usa updated:Nd, con N compreso tra 1 e 36500".to_owned())?;
                    query.updated_days = Some(days);
                }
                "has" if value.eq_ignore_ascii_case("attachment") => {
                    query.has_attachment = true;
                }
                "has" if value.eq_ignore_ascii_case("task") => query.has_open_task = true,
                "has" => return Err(format!("Filtro has:{value} non riconosciuto")),
                "is" if value.eq_ignore_ascii_case("pinned") => query.is_pinned = true,
                "is" if value.eq_ignore_ascii_case("archived") => query.is_archived = true,
                "is" if value.eq_ignore_ascii_case("task") => query.has_open_task = true,
                "is" => return Err(format!("Filtro is:{value} non riconosciuto")),
                _ => return Err(format!("Filtro {field}: non riconosciuto")),
            }
        } else {
            query.terms.push(token.value);
            query.quoted.push(token.quoted);
        }
    }

    Ok(query)
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut token_quoted = false;
    let mut escaped = false;

    for character in input.chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' && quoted {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
            token_quoted = true;
        } else if !quoted && matches!(character, '(' | ')') {
            if !current.is_empty() {
                tokens.push(Token {
                    value: std::mem::take(&mut current),
                    quoted: token_quoted,
                });
                token_quoted = false;
            }
            tokens.push(Token {
                value: character.to_string(),
                quoted: false,
            });
        } else if character.is_whitespace() && !quoted {
            if !current.is_empty() {
                tokens.push(Token {
                    value: std::mem::take(&mut current),
                    quoted: token_quoted,
                });
                token_quoted = false;
            }
        } else {
            current.push(character);
        }
    }

    if quoted {
        return Err("Manca la virgoletta di chiusura nella ricerca".to_owned());
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        tokens.push(Token {
            value: current,
            quoted: token_quoted,
        });
    }
    Ok(tokens)
}

fn validate_date(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return Err(format!("Data non valida: {value}; usa AAAA-MM-GG"));
    }
    let year = value[0..4].parse::<u32>().unwrap_or(0);
    let month = value[5..7].parse::<u32>().unwrap_or(0);
    let day = value[8..10].parse::<u32>().unwrap_or(0);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0 || day == 0 || day > days_in_month {
        return Err(format!("Data non valida: {value}; usa una data reale"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SearchQuery, parse_query};

    #[test]
    fn parses_search_terms_and_supported_filters() {
        assert_eq!(
            parse_query(
                r#"riunione "piano annuale" tag:progetto notebook:personale space:lavoro created:2026-01-01..2026-12-31 updated:7d has:attachment is:pinned is:task before:2026-10-01 after:2026-01-01"#
            )
            .expect("query parses"),
            SearchQuery {
                terms: vec!["riunione".to_owned(), "piano annuale".to_owned()],
                quoted: vec![false, true],
                tags: vec!["progetto".to_owned()],
                notebooks: vec!["personale".to_owned()],
                spaces: vec!["lavoro".to_owned()],
                created_range: Some(("2026-01-01".to_owned(), "2026-12-31".to_owned())),
                before: Some("2026-10-01".to_owned()),
                after: Some("2026-01-01".to_owned()),
                updated_days: Some(7),
                has_attachment: true,
                is_pinned: true,
                is_archived: false,
                has_open_task: true,
            }
        );
    }

    #[test]
    fn reports_malformed_and_unsupported_queries() {
        for query in [
            r#""frase incompleta"#,
            "created:2026-02-30..2026-03-01",
            "updated:0d",
            "has:tasks",
            "is:done",
            "AND progetto",
            "progetto AND (nota OR piano)",
            "progetto (nota)",
            "stato:aperto",
        ] {
            assert!(parse_query(query).is_err(), "query should fail: {query}");
        }
    }

    #[test]
    fn safely_preserves_quoted_special_characters() {
        assert_eq!(
            parse_query(r#""C++ (piano) \"bozza\"""#).expect("phrase parses"),
            SearchQuery {
                terms: vec![r#"C++ (piano) "bozza""#.to_owned()],
                quoted: vec![true],
                ..SearchQuery::default()
            }
        );
    }
}
