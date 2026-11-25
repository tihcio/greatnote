//! Sintassi della ricerca: parole, frasi tra virgolette, filtri `campo:valore`
//! e, facoltativi, gli operatori `AND`, `OR`, `NOT` con le parentesi.
//!
//! Precedenza: `NOT` lega più di `AND` (anche implicito, con lo spazio), che
//! lega più di `OR`. Gli operatori valgono solo in maiuscolo: «e», «or»
//! minuscoli restano parole. Il testo dell'utente non diventa mai SQL: la
//! traduzione in `storage.rs` passa ogni valore come parametro.

/// Profondità massima delle parentesi.
const MAX_DEPTH: usize = 8;

/// Filtro di una ricerca (`tag:`, `notebook:`, `is:`…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    Tag(String),
    Notebook(String),
    Space(String),
    Created(String, String),
    Before(String),
    After(String),
    UpdatedDays(u32),
    HasAttachment,
    Pinned,
    Archived,
    OpenTask,
}

/// Elemento di un'espressione booleana.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Parola (anche come prefisso) o frase esatta tra virgolette.
    Term {
        text: String,
        quoted: bool,
    },
    Filter(Filter),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

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
    /// Espressione con operatori booleani o parentesi. Se presente, i campi
    /// sopra non sono filtri: `terms` elenca le parole non negate (per
    /// l'assistente AI) e `is_archived` dice se compare `is:archived`.
    pub expression: Option<Expr>,
}

impl SearchQuery {
    /// Filtri di una ricerca senza operatori, tutti da soddisfare.
    pub fn flat_filters(&self) -> Vec<Filter> {
        let mut filters: Vec<Filter> = Vec::new();
        filters.extend(self.tags.iter().cloned().map(Filter::Tag));
        filters.extend(self.notebooks.iter().cloned().map(Filter::Notebook));
        filters.extend(self.spaces.iter().cloned().map(Filter::Space));
        if let Some((start, end)) = &self.created_range {
            filters.push(Filter::Created(start.clone(), end.clone()));
        }
        if let Some(before) = &self.before {
            filters.push(Filter::Before(before.clone()));
        }
        if let Some(after) = &self.after {
            filters.push(Filter::After(after.clone()));
        }
        if let Some(days) = self.updated_days {
            filters.push(Filter::UpdatedDays(days));
        }
        if self.has_attachment {
            filters.push(Filter::HasAttachment);
        }
        if self.is_pinned {
            filters.push(Filter::Pinned);
        }
        if self.is_archived {
            filters.push(Filter::Archived);
        }
        if self.has_open_task {
            filters.push(Filter::OpenTask);
        }
        filters
    }
}

#[derive(Debug)]
struct Token {
    value: String,
    quoted: bool,
}

impl Token {
    /// Operatore o parentesi, se il token non è tra virgolette.
    fn operator(&self) -> Option<&'static str> {
        if self.quoted {
            return None;
        }
        match self.value.as_str() {
            "AND" => Some("AND"),
            "OR" => Some("OR"),
            "NOT" => Some("NOT"),
            "(" => Some("("),
            ")" => Some(")"),
            _ => None,
        }
    }
}

pub fn parse_query(input: &str) -> Result<SearchQuery, String> {
    if input.chars().count() > 512 {
        return Err(crate::tr!(
            "La ricerca non può superare 512 caratteri",
            "Search cannot exceed 512 characters"
        ));
    }

    let tokens = tokenize(input)?;
    if tokens.len() > 32 {
        return Err(crate::tr!(
            "La ricerca può contenere al massimo 32 termini o filtri",
            "Search can contain at most 32 terms or filters"
        ));
    }

    let boolean = tokens.iter().any(|token| token.operator().is_some());
    if boolean {
        let mut parser = Parser {
            tokens: &tokens,
            position: 0,
        };
        let expression = parser.or(0)?;
        if let Some(token) = tokens.get(parser.position) {
            return Err(match token.operator() {
                Some(")") => crate::tr!(
                    "Parentesi di chiusura senza apertura nella ricerca",
                    "Closing parenthesis without an opening one in search"
                ),
                _ => crate::tr!(
                    "Ricerca non valida vicino a «{}»",
                    "Invalid search near “{}”",
                    token.value
                ),
            });
        }
        let mut query = SearchQuery::default();
        collect_positive(&expression, false, &mut query);
        query.expression = Some(expression);
        return Ok(query);
    }

    let mut query = SearchQuery::default();
    for token in tokens {
        match parse_atom(&token)? {
            Expr::Term { text, quoted } => {
                query.terms.push(text);
                query.quoted.push(quoted);
            }
            Expr::Filter(filter) => match filter {
                Filter::Tag(value) => query.tags.push(value),
                Filter::Notebook(value) => query.notebooks.push(value),
                Filter::Space(value) => query.spaces.push(value),
                Filter::Created(start, end) => query.created_range = Some((start, end)),
                Filter::Before(value) => query.before = Some(value),
                Filter::After(value) => query.after = Some(value),
                Filter::UpdatedDays(days) => query.updated_days = Some(days),
                Filter::HasAttachment => query.has_attachment = true,
                Filter::Pinned => query.is_pinned = true,
                Filter::Archived => query.is_archived = true,
                Filter::OpenTask => query.has_open_task = true,
            },
            _ => {}
        }
    }
    Ok(query)
}

/// Parole e `is:archived` non negati di un'espressione.
fn collect_positive(expression: &Expr, negated: bool, query: &mut SearchQuery) {
    match expression {
        Expr::Term { text, quoted } if !negated => {
            query.terms.push(text.clone());
            query.quoted.push(*quoted);
        }
        Expr::Filter(Filter::Archived) if !negated => query.is_archived = true,
        Expr::Not(inner) => collect_positive(inner, !negated, query),
        Expr::And(items) | Expr::Or(items) => {
            for item in items {
                collect_positive(item, negated, query);
            }
        }
        _ => {}
    }
}

/// Analisi a discesa ricorsiva: `or := and (OR and)*`,
/// `and := unary ([AND] unary)*`, `unary := NOT unary | primary`,
/// `primary := ( or ) | termine | filtro`.
struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn peek_operator(&self) -> Option<&'static str> {
        self.peek().and_then(Token::operator)
    }

    fn missing_operand(&self, operator: &str) -> String {
        crate::tr!(
            "Manca un termine dopo «{}» nella ricerca",
            "Missing a term after “{}” in search",
            operator
        )
    }

    fn or(&mut self, depth: usize) -> Result<Expr, String> {
        let mut items = vec![self.and(depth)?];
        while self.peek_operator() == Some("OR") {
            self.position += 1;
            if self.peek().is_none() || matches!(self.peek_operator(), Some(")" | "OR" | "AND")) {
                return Err(self.missing_operand("OR"));
            }
            items.push(self.and(depth)?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Expr::Or(items)
        })
    }

    fn and(&mut self, depth: usize) -> Result<Expr, String> {
        let mut items = vec![self.unary(depth)?];
        loop {
            match self.peek_operator() {
                Some("AND") => {
                    self.position += 1;
                    if self.peek().is_none()
                        || matches!(self.peek_operator(), Some(")" | "OR" | "AND"))
                    {
                        return Err(self.missing_operand("AND"));
                    }
                }
                Some("OR" | ")") => break,
                _ if self.peek().is_none() => break,
                _ => {}
            }
            items.push(self.unary(depth)?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Expr::And(items)
        })
    }

    fn unary(&mut self, depth: usize) -> Result<Expr, String> {
        if self.peek_operator() == Some("NOT") {
            self.position += 1;
            if self.peek().is_none() || matches!(self.peek_operator(), Some(")" | "OR" | "AND")) {
                return Err(self.missing_operand("NOT"));
            }
            return Ok(Expr::Not(Box::new(self.unary(depth)?)));
        }
        self.primary(depth)
    }

    fn primary(&mut self, depth: usize) -> Result<Expr, String> {
        let Some(token) = self.peek() else {
            return Err(crate::tr!(
                "La ricerca finisce con un operatore",
                "Search ends with an operator"
            ));
        };
        match token.operator() {
            Some("(") => {
                if depth >= MAX_DEPTH {
                    return Err(crate::tr!(
                        "Troppe parentesi annidate nella ricerca (al massimo {})",
                        "Too many nested parentheses in search (at most {})",
                        MAX_DEPTH
                    ));
                }
                self.position += 1;
                if self.peek_operator() == Some(")") {
                    return Err(crate::tr!(
                        "Parentesi vuote nella ricerca",
                        "Empty parentheses in search"
                    ));
                }
                let inner = self.or(depth + 1)?;
                if self.peek_operator() != Some(")") {
                    return Err(crate::tr!(
                        "Manca la parentesi di chiusura nella ricerca",
                        "Missing closing parenthesis in search"
                    ));
                }
                self.position += 1;
                Ok(inner)
            }
            Some(")") => Err(crate::tr!(
                "Parentesi di chiusura senza apertura nella ricerca",
                "Closing parenthesis without an opening one in search"
            )),
            Some(operator) => Err(crate::tr!(
                "Manca un termine prima di «{}» nella ricerca",
                "Missing a term before “{}” in search",
                operator
            )),
            None => {
                let atom = parse_atom(token)?;
                self.position += 1;
                Ok(atom)
            }
        }
    }
}

/// Parola, frase o filtro `campo:valore`.
fn parse_atom(token: &Token) -> Result<Expr, String> {
    if token.quoted {
        return Ok(Expr::Term {
            text: token.value.clone(),
            quoted: true,
        });
    }
    let Some((field, value)) = token.value.split_once(':') else {
        return Ok(Expr::Term {
            text: token.value.clone(),
            quoted: false,
        });
    };
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::tr!(
            "Il filtro {field}: richiede un valore",
            "Filter {field}: requires a value"
        ));
    }
    let filter = match field.to_ascii_lowercase().as_str() {
        "tag" => Filter::Tag(value.to_owned()),
        "notebook" => Filter::Notebook(value.to_owned()),
        "space" => Filter::Space(value.to_owned()),
        "created" => {
            let (start, end) = value
                .split_once("..")
                .ok_or_else(|| "Usa created:AAAA-MM-GG..AAAA-MM-GG".to_owned())?;
            validate_date(start)?;
            validate_date(end)?;
            if start > end {
                return Err(crate::tr!(
                    "L'intervallo created ha le date invertite",
                    "The created range has inverted dates"
                ));
            }
            Filter::Created(start.to_owned(), end.to_owned())
        }
        "before" => {
            validate_date(value)?;
            Filter::Before(value.to_owned())
        }
        "after" => {
            validate_date(value)?;
            Filter::After(value.to_owned())
        }
        "updated" => {
            let days = value
                .strip_suffix('d')
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|days| (1..=36_500).contains(days))
                .ok_or_else(|| {
                    crate::tr!(
                        "Usa updated:Nd, con N compreso tra 1 e 36500",
                        "Use updated:Nd, with N between 1 and 36500"
                    )
                })?;
            Filter::UpdatedDays(days)
        }
        "has" if value.eq_ignore_ascii_case("attachment") => Filter::HasAttachment,
        "has" if value.eq_ignore_ascii_case("task") => Filter::OpenTask,
        "has" => {
            return Err(crate::tr!(
                "Filtro has:{value} non riconosciuto",
                "Unrecognized filter has:{value}"
            ));
        }
        "is" if value.eq_ignore_ascii_case("pinned") => Filter::Pinned,
        "is" if value.eq_ignore_ascii_case("archived") => Filter::Archived,
        "is" if value.eq_ignore_ascii_case("task") => Filter::OpenTask,
        "is" => {
            return Err(crate::tr!(
                "Filtro is:{value} non riconosciuto",
                "Unrecognized filter is:{value}"
            ));
        }
        _ => {
            return Err(crate::tr!(
                "Filtro {field}: non riconosciuto",
                "Unrecognized filter {field}"
            ));
        }
    };
    Ok(Expr::Filter(filter))
}

/// Testo per confronti senza maiuscole né accenti: minuscolo Unicode e
/// lettere latine accentate ridotte alla lettera base («Città» → «citta»).
/// Usato dalla ricerca nelle attività come funzione SQL `gn_fold`.
pub fn fold_text(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|character| match character {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
            'ď' | 'đ' => 'd',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
            'ĥ' | 'ħ' => 'h',
            'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
            'ĵ' => 'j',
            'ķ' => 'k',
            'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
            'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
            'ŕ' | 'ŗ' | 'ř' => 'r',
            'ś' | 'ŝ' | 'ş' | 'š' => 's',
            'ţ' | 'ť' | 'ŧ' => 't',
            'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
            'ŵ' => 'w',
            'ý' | 'ÿ' | 'ŷ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            other => other,
        })
        .collect()
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
        return Err(crate::tr!(
            "Manca la virgoletta di chiusura nella ricerca",
            "Missing closing quote in search"
        ));
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
        return Err(crate::tr!(
            "Data non valida: {value}; usa AAAA-MM-GG",
            "Invalid date: {value}; use YYYY-MM-DD"
        ));
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
        return Err(crate::tr!(
            "Data non valida: {value}; usa una data reale",
            "Invalid date: {value}; use a real date"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Expr, Filter, SearchQuery, fold_text, parse_query};

    fn term(text: &str) -> Expr {
        Expr::Term {
            text: text.to_owned(),
            quoted: false,
        }
    }

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
                expression: None,
            }
        );
    }

    #[test]
    fn boolean_operators_follow_precedence_and_parentheses() {
        // NOT > AND (anche implicito) > OR.
        let query = parse_query("progetto OR piano bozza NOT tag:vecchio").expect("parses");
        assert_eq!(
            query.expression,
            Some(Expr::Or(vec![
                term("progetto"),
                Expr::And(vec![
                    term("piano"),
                    term("bozza"),
                    Expr::Not(Box::new(Expr::Filter(Filter::Tag("vecchio".to_owned())))),
                ]),
            ]))
        );
        // Le parole non negate restano disponibili (assistente AI).
        assert_eq!(query.terms, ["progetto", "piano", "bozza"]);
        let grouped = parse_query("(progetto OR piano) AND \"budget 2026\"").expect("parses");
        assert_eq!(
            grouped.expression,
            Some(Expr::And(vec![
                Expr::Or(vec![term("progetto"), term("piano")]),
                Expr::Term {
                    text: "budget 2026".to_owned(),
                    quoted: true,
                },
            ]))
        );
        // Minuscoli o tra virgolette non sono operatori.
        let words = parse_query("cane or gatto \"AND\"").expect("parses");
        assert!(words.expression.is_none());
        assert_eq!(words.terms, ["cane", "or", "gatto", "AND"]);
        assert!(!parse_query("NOT is:archived").expect("parses").is_archived);
        assert!(
            parse_query("tag:x OR is:archived")
                .expect("parses")
                .is_archived
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
            "progetto AND",
            "progetto OR",
            "NOT",
            "progetto OR OR piano",
            "(progetto",
            "progetto)",
            "()",
            "((((((((( a )))))))))",
            "NOT tag:",
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

    #[test]
    fn folding_ignores_case_and_latin_accents() {
        assert_eq!(
            fold_text("Città ÉTÉ Straße Ñandú"),
            "citta ete straße nandu"
        );
        assert_eq!(fold_text("ŁÓDŹ"), "lodz");
    }
}
