//! Articolo di una pagina web condivisa con Greatnote: la pagina viene
//! scaricata e ne viene estratto il contenuto principale (testo, titoli,
//! elenchi, tabelle, citazioni), senza menu, piè di pagina, commenti e
//! pubblicità. La conversione finale è quella dell'importazione HTML, che
//! tiene solo i nodi ammessi e i link sicuri.

use std::time::Duration;

use scraper::{ElementRef, Html, Node, Selector};
use serde_json::{Value, json};
use ureq::Agent;

use crate::content::html_to_document;

/// Pagina più grande scaricata.
const MAX_PAGE_BYTES: u64 = 5 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
/// Sotto questa lunghezza il testo estratto non è considerato un articolo
/// (pagine a pagamento, costruite in JavaScript, di solo menu…).
const MIN_ARTICLE_CHARS: usize = 200;
const MAX_TITLE_CHARS: usize = 200;

/// Articolo estratto da una pagina.
#[derive(Debug, Clone, PartialEq)]
pub struct Article {
    pub title: Option<String>,
    /// Documento Tiptap con il collegamento alla fonte in cima.
    pub document: Value,
}

/// Primo indirizzo `http`/`https` nel testo condiviso.
pub fn find_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | '"' | '\'')))
        .find(|word| {
            let lower = word.to_ascii_lowercase();
            (lower.starts_with("https://") || lower.starts_with("http://")) && word.len() > 10
        })
        .map(|word| {
            word.trim_end_matches(['.', ',', ';', ':', '!', '?'])
                .to_owned()
        })
}

/// Scarica la pagina e ne estrae l'articolo.
pub fn fetch_article(url: &str) -> Result<Article, String> {
    fetch_article_following(url, 2)
}

/// Scarica la pagina; una pagina di solo reindirizzamento (`<meta
/// http-equiv="refresh">`) viene seguita al massimo `hops` volte.
fn fetch_article_following(url: &str, hops: u8) -> Result<Article, String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(crate::tr!(
            "indirizzo non supportato",
            "unsupported address"
        ));
    }
    let agent: Agent = Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(TIMEOUT))
        .timeout_recv_body(Some(TIMEOUT))
        .user_agent(concat!(
            "Mozilla/5.0 (compatible; Greatnote/",
            env!("CARGO_PKG_VERSION"),
            ")"
        ))
        .build()
        .new_agent();
    let mut response = agent
        .get(url)
        .header("Accept", "text/html,application/xhtml+xml")
        .call()
        .map_err(|error| match error {
            ureq::Error::Timeout(_) => crate::tr!("tempo scaduto", "timed out"),
            ureq::Error::HostNotFound => crate::tr!(
                "sito non raggiungibile: sei connesso a internet?",
                "site unreachable: are you connected to the internet?"
            ),
            other => other.to_string(),
        })?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(crate::tr!(
            "il sito ha risposto con l'errore {}",
            "the site answered with error {}",
            status
        ));
    }
    let declared_html = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("html"));
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_PAGE_BYTES)
        .read_to_vec()
        .map_err(|error| match error {
            ureq::Error::BodyExceedsLimit(_) => {
                crate::tr!("pagina troppo grande", "page too large")
            }
            other => other.to_string(),
        })?;
    let html = String::from_utf8_lossy(&bytes);
    if !declared_html && !looks_like_html(&html) {
        return Err(crate::tr!(
            "l'indirizzo non è una pagina web",
            "the address is not a web page"
        ));
    }
    if hops > 0
        && let Some(target) = meta_refresh_target(&html)
    {
        return fetch_article_following(&target, hops - 1);
    }
    extract_article(&html, url)
}

/// Destinazione di un `<meta http-equiv="refresh" content="0; url=…">`
/// assoluta `http`/`https`.
fn meta_refresh_target(html: &str) -> Option<String> {
    // Ricerca nel testo: dentro `<noscript>` il parser HTML (con gli script
    // abilitati) non crea elementi.
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(offset) = lower[from..].find("<meta") {
        let start = from + offset;
        let end = start + lower[start..].find('>')?;
        from = end;
        let tag = &lower[start..end];
        if !tag.contains("http-equiv") || !tag.contains("refresh") {
            continue;
        }
        let content_at = tag.find("content=")? + "content=".len();
        let original = &html[start + content_at..end];
        let quote = original.chars().next()?;
        let value = if matches!(quote, '"' | '\'') {
            original[1..].split(quote).next()?
        } else {
            original.split_whitespace().next()?
        };
        let url_at = value.to_ascii_lowercase().find("url=")? + "url=".len();
        let target = value[url_at..].trim().trim_matches(['\'', '"']);
        return find_url(target);
    }
    None
}

fn looks_like_html(text: &str) -> bool {
    let start: String = text
        .trim_start()
        .chars()
        .take(64)
        .collect::<String>()
        .to_ascii_lowercase();
    start.starts_with("<!doctype html") || start.starts_with("<html")
}

/// Elementi che non fanno parte dell'articolo.
fn is_boilerplate(element: ElementRef<'_>) -> bool {
    let value = element.value();
    if matches!(
        value.name(),
        "script"
            | "style"
            | "noscript"
            | "nav"
            | "aside"
            | "footer"
            | "header"
            | "form"
            | "button"
            | "iframe"
            | "svg"
            | "template"
            | "dialog"
            | "select"
            | "input"
            | "textarea"
    ) {
        return true;
    }
    if value.attr("aria-hidden") == Some("true") || value.attr("hidden").is_some() {
        return true;
    }
    if matches!(
        value.attr("role"),
        Some("navigation" | "banner" | "contentinfo" | "complementary")
    ) {
        return true;
    }
    let names = format!(
        "{} {}",
        value.attr("class").unwrap_or_default(),
        value.attr("id").unwrap_or_default()
    )
    .to_ascii_lowercase();
    const NOISE: [&str; 16] = [
        "comment",
        "share",
        "social",
        "related",
        "promo",
        "advert",
        "cookie",
        "newsletter",
        "sidebar",
        "menu",
        "breadcrumb",
        "subscribe",
        "banner",
        "popup",
        "outbrain",
        "taboola",
    ];
    NOISE.iter().any(|word| names.contains(word))
}

/// Testo dei paragrafi di un elemento (misura di "quanto articolo" c'è).
fn paragraph_score(element: ElementRef<'_>) -> usize {
    let paragraphs = Selector::parse("p").expect("selettore costante");
    element
        .select(&paragraphs)
        .map(|paragraph| paragraph.text().map(str::trim).collect::<String>().len())
        .filter(|length| *length > 40)
        .sum()
}

/// Contenitore dell'articolo: `<article>` o `<main>` se contengono testo,
/// altrimenti il blocco con più testo nei paragrafi.
fn main_container(document: &Html) -> Option<ElementRef<'_>> {
    for selector in ["article", "main", "[role=main]", "[itemprop=articleBody]"] {
        let selector = Selector::parse(selector).expect("selettore costante");
        if let Some(best) = document
            .select(&selector)
            .max_by_key(|element| paragraph_score(*element))
            .filter(|element| paragraph_score(*element) >= MIN_ARTICLE_CHARS)
        {
            return Some(best);
        }
    }
    let blocks = Selector::parse("div, section, td").expect("selettore costante");
    document
        .select(&blocks)
        .filter(|element| !is_boilerplate(*element))
        .max_by_key(|element| {
            // Il blocco con più testo diretto nei suoi paragrafi figli: un
            // contenitore esterno non vince solo perché li contiene tutti.
            element
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|child| child.value().name() == "p")
                .map(|paragraph| paragraph.text().collect::<String>().len())
                .sum::<usize>()
        })
}

/// HTML ripulito del contenitore: elementi di disturbo tolti, attributi
/// ridotti a quelli che la conversione usa.
fn clean_html(element: ElementRef<'_>, output: &mut String, depth: usize) {
    if depth > 200 {
        return;
    }
    for child in element.children() {
        match child.value() {
            Node::Text(text) => output.push_str(&escape(text)),
            Node::Element(_) => {
                let Some(child) = ElementRef::wrap(child) else {
                    continue;
                };
                if is_boilerplate(child) {
                    continue;
                }
                let name = child.value().name();
                let mut attributes = String::new();
                for attribute in [
                    "href", "alt", "colspan", "rowspan", "start", "type", "checked",
                ] {
                    if let Some(value) = child.value().attr(attribute) {
                        attributes.push_str(&format!(" {attribute}=\"{}\"", escape(value)));
                    }
                }
                output.push_str(&format!("<{name}{attributes}>"));
                clean_html(child, output, depth + 1);
                output.push_str(&format!("</{name}>"));
            }
            _ => {}
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn meta(document: &Html, selector: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    document
        .select(&selector)
        .find_map(|element| element.value().attr("content"))
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
}

fn page_title(document: &Html) -> Option<String> {
    let title = meta(document, "meta[property='og:title']").or_else(|| {
        let selector = Selector::parse("title").ok()?;
        document
            .select(&selector)
            .next()
            .map(|title| title.text().collect::<Vec<_>>().join(" "))
            .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|value| !value.is_empty())
    })?;
    Some(title.chars().take(MAX_TITLE_CHARS).collect())
}

/// Estrae l'articolo dall'HTML della pagina `url`.
pub fn extract_article(html: &str, url: &str) -> Result<Article, String> {
    let page = Html::parse_document(html);
    let title = page_title(&page);
    let container = main_container(&page).ok_or_else(|| {
        crate::tr!(
            "nella pagina non c'è un articolo leggibile",
            "the page has no readable article"
        )
    })?;
    let mut cleaned = String::new();
    clean_html(container, &mut cleaned, 0);
    let parsed = html_to_document(&format!("<html><body>{cleaned}</body></html>"));
    let mut blocks = parsed
        .document
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let text_length: usize = crate::content::to_plain_text(&parsed.document)
        .chars()
        .count();
    if text_length < MIN_ARTICLE_CHARS {
        return Err(crate::tr!(
            "nella pagina non c'è un articolo leggibile (forse richiede un abbonamento o JavaScript)",
            "the page has no readable article (it may need a subscription or JavaScript)"
        ));
    }
    // Un titolo che ripete quello della pagina diventa superfluo.
    if let (Some(first), Some(title)) = (blocks.first(), &title)
        && first.get("type").and_then(Value::as_str) == Some("heading")
        && crate::content::to_plain_text(&json!({ "type": "doc", "content": [first] }))
            .trim()
            .eq_ignore_ascii_case(title.trim())
    {
        blocks.remove(0);
    }
    if let Some(title) = &parsed.title
        && blocks.is_empty()
    {
        blocks.push(json!({ "type": "paragraph", "content": [{ "type": "text", "text": title }] }));
    }
    let source = json!({ "type": "paragraph", "content": [
        { "type": "text", "text": crate::tr!("Fonte: ", "Source: ") },
        { "type": "text", "text": url, "marks": [{ "type": "link", "attrs": { "href": url } }] }
    ]});
    let mut content = vec![source, json!({ "type": "horizontalRule" })];
    content.extend(blocks);
    Ok(Article {
        title,
        document: json!({ "type": "doc", "content": content }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::note::validate_document;

    const ARTICLE: &str = "Il comune ha presentato oggi il nuovo piano per la mobilità sostenibile, che prevede piste ciclabili e una rete di autobus elettrici entro il 2028.";

    fn page(body: &str) -> String {
        format!(
            "<!doctype html><html><head><title>Sito | Pagina</title>\
             <meta property=\"og:title\" content=\"Piano per la mobilità\"></head><body>{body}</body></html>"
        )
    }

    #[test]
    fn finds_the_shared_address() {
        assert_eq!(
            find_url("Piano per la mobilità\nhttps://example.com/articolo?id=3."),
            Some("https://example.com/articolo?id=3".to_owned())
        );
        assert_eq!(
            find_url("(http://example.org/x)"),
            Some("http://example.org/x".to_owned())
        );
        assert_eq!(find_url("nessun link, solo ftp://x.y"), None);
    }

    #[test]
    fn extracts_the_article_without_boilerplate() {
        let html = page(&format!(
            "<header><nav><a href='/'>Home</a> Menu principale</nav></header>\
             <div class='cookie-banner'>Accetta i cookie</div>\
             <article><h1>Piano per la mobilità</h1>\
               <p>{ARTICLE}</p><h2>I tempi</h2><p>{ARTICLE} <a href='https://example.com/doc'>Documento</a></p>\
               <ul><li>Piste ciclabili</li><li>Autobus elettrici</li></ul>\
               <div class='share-buttons'>Condividi su Facebook</div>\
               <script>tracker()</script>\
             </article>\
             <aside>Articoli correlati</aside><footer>© Editore</footer>"
        ));
        let article = extract_article(&html, "https://example.com/articolo").expect("article");
        assert_eq!(article.title.as_deref(), Some("Piano per la mobilità"));
        validate_document(&article.document).expect("valid document");
        let text = crate::content::to_plain_text(&article.document);
        assert!(
            text.starts_with("Fonte: https://example.com/articolo"),
            "{text}"
        );
        assert!(
            text.contains("I tempi") && text.contains("Autobus elettrici"),
            "{text}"
        );
        for noise in [
            "Menu principale",
            "cookie",
            "Facebook",
            "tracker",
            "correlati",
            "Editore",
        ] {
            assert!(!text.contains(noise), "{noise} in {text}");
        }
        // Il titolo uguale a quello della pagina non viene ripetuto.
        assert_eq!(text.matches("Piano per la mobilità").count(), 0);
        let serialized = article.document.to_string();
        assert!(serialized.contains("https://example.com/doc"));
        assert!(serialized.contains("\"bulletList\""));
    }

    #[test]
    fn pages_without_article_markup_use_the_densest_block() {
        let html = page(&format!(
            "<div id='menu'><p>Voce di menu molto lunga per sembrare un paragrafo di testo vero</p></div>\
             <div class='contenuto'><p>{ARTICLE}</p><p>{ARTICLE}</p></div>"
        ));
        let article = extract_article(&html, "https://example.com/x").expect("article");
        let text = crate::content::to_plain_text(&article.document);
        assert!(text.contains("mobilità sostenibile"));
        assert!(!text.contains("Voce di menu"));
    }

    #[test]
    fn short_or_script_only_pages_are_refused() {
        let html = page("<div id='app'></div><script>render()</script><p>Abbonati per leggere</p>");
        assert!(extract_article(&html, "https://example.com/x").is_err());
        assert!(fetch_article("file:///etc/passwd").is_err());
        assert!(fetch_article("javascript:alert(1)").is_err());
    }

    #[test]
    fn meta_refresh_pages_point_to_their_target() {
        let redirect = r#"<!doctype html><meta charset="utf-8"><noscript><meta http-equiv="refresh" content="0; url=https://blog.example.com/post/"></noscript><p><a href="x">Click</a></p>"#;
        assert_eq!(
            meta_refresh_target(redirect).as_deref(),
            Some("https://blog.example.com/post/")
        );
        let quoted = r#"<meta http-equiv="Refresh" content="5;URL='http://example.org/a'">"#;
        assert_eq!(
            meta_refresh_target(quoted).as_deref(),
            Some("http://example.org/a")
        );
        assert_eq!(
            meta_refresh_target(r#"<meta http-equiv="refresh" content="0; url=javascript:x">"#),
            None
        );
        assert_eq!(meta_refresh_target("<p>nessun reindirizzamento</p>"), None);
    }

    #[test]
    fn downloads_and_extracts_over_http() {
        let html = page(&format!("<main><p>{ARTICLE}</p><p>{ARTICLE}</p></main>"));
        let server = crate::calendar_sync::http::fake::FakeServer::start(vec![(200, html)]);
        let article = fetch_article(&format!("{}/articolo", server.url)).expect("article");
        assert!(crate::content::to_plain_text(&article.document).contains("autobus elettrici"));
        let failing =
            crate::calendar_sync::http::fake::FakeServer::start(vec![(404, String::new())]);
        let error = fetch_article(&format!("{}/manca", failing.url)).expect_err("404");
        assert!(error.contains("404"), "{error}");
    }

    /// Prova su una pagina vera, ignorata di default (richiede la rete):
    /// `GREATNOTE_CLIP_URL=https://… cargo test -p greatnote-core live_page -- --ignored --nocapture`
    #[test]
    #[ignore = "richiede la rete e GREATNOTE_CLIP_URL"]
    fn live_page_extraction() {
        let url = std::env::var("GREATNOTE_CLIP_URL").expect("GREATNOTE_CLIP_URL");
        let article = fetch_article(&url).expect("article");
        let text = crate::content::to_plain_text(&article.document);
        println!(
            "TITOLO: {:?}\n{}",
            article.title,
            text.chars().take(1500).collect::<String>()
        );
        println!("… {} caratteri in tutto", text.chars().count());
    }
}
