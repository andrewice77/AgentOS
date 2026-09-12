//! Opt-in web grounding: DuckDuckGo HTML search + fetch of top pages.
//! The local LLM never chooses URLs; the app retrieves evidence, then the model summarizes.

use crate::organizer;
use futures_util::future::join_all;
use regex::Regex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::OnceLock;
use std::time::Duration;

const SEARCH_URL: &str = "https://html.duckduckgo.com/html/";
const SEARCH_LITE_URL: &str = "https://lite.duckduckgo.com/lite/";
const USER_AGENT: &str = "AgentOS/0.1 (local-first desktop assistant; +https://github.com/andrewice/agentos) Mozilla/5.0 (X11; Linux x86_64) Chrome/122.0.0.0 Safari/537.36";
const MAX_RESULTS: usize = 5;
const MAX_PAGES: usize = 2;
const MAX_SNIPPET_CHARS: usize = 280;
const MAX_PAGE_CHARS: usize = 1800;
const MAX_BODY_BYTES: usize = 400_000;
const SEARCH_TIMEOUT: Duration = Duration::from_secs(12);
const PAGE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub page_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WebEvidence {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebCommand {
    Help,
    Query(String),
}

#[derive(Debug, Clone)]
pub struct WebThread {
    pub session_id: String,
    pub subject: String,
    pub constraints: String,
}

pub fn parse_command(message: &str) -> Option<WebCommand> {
    let trimmed = message.trim();
    let lower = trimmed.to_lowercase();
    let prefixes = [
        "cerca online",
        "cerca sul web",
        "cerca in rete",
        "search online",
        "web search",
    ];
    for prefix in prefixes {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if !(rest.is_empty() || rest.starts_with(':') || rest.starts_with(' ')) {
                continue;
            }
            let query = trimmed
                .get(prefix.len()..)
                .unwrap_or("")
                .trim()
                .trim_start_matches(':')
                .trim();
            return Some(if query.is_empty() {
                WebCommand::Help
            } else {
                WebCommand::Query(query.to_string())
            });
        }
    }
    None
}

pub fn strip_question_shell(q: &str) -> String {
    let trimmed = q.trim().trim_end_matches(['?', '!', '.']).trim();
    let lower = trimmed.to_lowercase();
    let prefixes = [
        "chi è ",
        "chi e ",
        "chi era ",
        "who is ",
        "who was ",
        "cos'è ",
        "cose ",
        "what is ",
        "ultime notizie su ",
        "notizie su ",
    ];
    for p in prefixes {
        if let Some(rest) = lower.strip_prefix(p) {
            let skip = trimmed.len().saturating_sub(rest.len());
            return trimmed.get(skip..).unwrap_or(rest).trim().to_string();
        }
    }
    trimmed.to_string()
}

pub fn is_profile_request(msg: &str) -> bool {
    let l = msg.to_lowercase();
    [
        "linkedin",
        "github",
        "gitlab",
        "profilo",
        "profili",
        "link",
        "contatt",
        "email",
        "sito",
        "pagina",
        "url",
    ]
    .iter()
    .any(|k| l.contains(k))
}

pub fn is_web_followup(msg: &str) -> bool {
    if parse_command(msg).is_some() {
        return false;
    }
    if organizer::looks_like_task_create(msg) || organizer::looks_like_reminder_intent(msg) {
        return false;
    }
    let l = msg.to_lowercase();
    if l.starts_with("ricorda ")
        || l.starts_with("preferisco ")
        || l.starts_with("dimentica ")
        || l.starts_with("task:")
        || l.starts_with("mcp ")
        || l.starts_with("cerca:")
        || l.starts_with("cerca nelle")
        || l == "ok"
        || l == "ok."
        || l == "grazie"
        || l == "grazie."
    {
        return false;
    }
    const KEYS: &[&str] = &[
        "linkedin",
        "github",
        "gitlab",
        "profilo",
        "profili",
        "contatt",
        "email",
        "sito web",
        "sviluppatore",
        "php",
        "quello",
        "quella",
        "intendo",
        "sto cercando",
        "si sto",
        "sì sto",
        "più dettag",
        "piu dettag",
        "più info",
        "piu info",
        "approfond",
        "dammi",
        "fornisci",
        "i suoi",
        "i suoi ",
        "del suo",
        "della sua",
    ];
    if KEYS.iter().any(|k| l.contains(k)) {
        return true;
    }
    l.contains("link") && (l.contains("suo") || l.contains("profil") || l.contains("pagin"))
}

pub fn compose_followup_query(thread: &WebThread, followup: &str) -> String {
    let mut parts = vec![thread.subject.clone()];
    if !thread.constraints.is_empty() {
        parts.push(thread.constraints.clone());
    }
    if is_profile_request(followup) {
        parts.push("LinkedIn GitHub sito profilo".into());
    } else {
        let extra = strip_question_shell(followup);
        if !extra.is_empty() && !thread.subject.eq_ignore_ascii_case(&extra) {
            parts.push(extra);
        }
    }
    parts.join(" ")
}

pub fn apply_followup_constraint(thread: &mut WebThread, followup: &str) {
    if is_profile_request(followup) {
        return;
    }
    let extra = strip_question_shell(followup);
    if extra.is_empty() || extra.chars().count() > 80 {
        return;
    }
    if thread.subject.eq_ignore_ascii_case(&extra) {
        return;
    }
    let already = thread.constraints.to_lowercase();
    if already.contains(&extra.to_lowercase()) {
        return;
    }
    thread.constraints = if thread.constraints.is_empty() {
        extra
    } else {
        format!("{} {}", thread.constraints, extra)
    };
}

pub fn render_prompt_block(ev: &WebEvidence) -> String {
    let mut out = String::from(
        "\n\nWEB EVIDENCE (retrieved just now; may be incomplete or conflicting).\n\
         Answer using this evidence. Cite every relevant source as a markdown link.\n\
         Public professional profile URLs (LinkedIn, GitHub, GitLab, company pages, personal sites) \
         MUST be shared when present below — they are not private contact details.\n\
         Do NOT refuse to give those links. Do NOT tell the user to search by themselves if URLs are below.\n\
         Do NOT provide phone numbers, personal email addresses, home addresses, or private messages.\n\
         If the user asks how to contact someone: give public profile links from the evidence, \
         and say you do not have private contact details.\n\
         If sources disagree or several people match, say so and list the distinct profiles with links.\n\
         Do not invent URLs or facts that are not supported below.\n",
    );
    out.push_str(&format!("Query: {}\n", ev.query));
    if let Some(err) = &ev.error {
        out.push_str(&format!("WEB SEARCH NOTE: {err}\n"));
    }
    if ev.hits.is_empty() {
        out.push_str(
            "No usable web results. Tell the user you could not retrieve live sources. Do not invent a biography, news, or profile URLs.\n",
        );
        return out;
    }
    out.push_str("\nPublic URLs (copy these into the reply when relevant):\n");
    for hit in &ev.hits {
        out.push_str(&format!("- [{}]({})\n", hit.title, hit.url));
    }
    for (i, hit) in ev.hits.iter().enumerate() {
        out.push_str(&format!(
            "\n### Source {}\nTitle: {}\nURL: {}\nSnippet: {}\n",
            i + 1,
            hit.title,
            hit.url,
            hit.snippet
        ));
        if let Some(page) = &hit.page_text {
            out.push_str("Page extract:\n");
            out.push_str(page);
            out.push('\n');
        }
    }
    out
}

pub fn help_text() -> String {
    "Uso: `cerca online: chi è Andrea Scipio` oppure `cerca sul web: ultime notizie su …`.\n\
     AgentOS cerca sul web, legge 1–2 pagine e ti risponde dalle fonti. I follow-up \
     (es. «lo sviluppatore PHP», «dammi i link dei profili») rilanciano la ricerca."
        .into()
}

pub fn render_sources_footer(ev: &WebEvidence) -> String {
    if ev.hits.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\n---\n**Fonti trovate**\n");
    for (i, hit) in ev.hits.iter().enumerate() {
        out.push_str(&format!("{}. [{}]({})\n", i + 1, hit.title, hit.url));
    }
    out
}

pub async fn search_and_fetch(query: &str) -> WebEvidence {
    let query = query.trim();
    if query.is_empty() {
        return WebEvidence {
            query: String::new(),
            hits: vec![],
            error: Some("empty query".into()),
        };
    }

    let client = match http_client(SEARCH_TIMEOUT) {
        Ok(c) => c,
        Err(e) => {
            return WebEvidence {
                query: query.to_string(),
                hits: vec![],
                error: Some(e),
            };
        }
    };

    let mut hits = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut ddg_ok = false;

    match search_duckduckgo(&client, query).await {
        Ok(h) if !h.is_empty() => {
            ddg_ok = true;
            hits.extend(h);
        }
        Ok(_) => notes.push(
            "DuckDuckGo ha chiesto un captcha o non ha restituito risultati; uso fonti alternative."
                .into(),
        ),
        Err(e) => notes.push(format!("DuckDuckGo: {e}")),
    }

    if !ddg_ok {
        if let Ok(h) = ddg_instant_answer(&client, query).await {
            merge_hits(&mut hits, h);
        }
        if let Ok(h) = wikipedia_search(&client, "it", query).await {
            merge_hits(&mut hits, h);
        }
        if let Ok(h) = wikipedia_search(&client, "en", query).await {
            merge_hits(&mut hits, h);
        }
        if let Ok(h) = wikidata_search(&client, query).await {
            merge_hits(&mut hits, h);
        }
        if let Ok(h) = news_rss(&client, query).await {
            merge_hits(&mut hits, h);
        }
        hits.sort_by_key(|h| source_rank(&h.url, query));
    } else if query.to_lowercase().contains("linkedin")
        || query.to_lowercase().contains("github")
        || query.to_lowercase().contains("profilo")
    {
        hits.sort_by_key(|h| source_rank(&h.url, query));
    }

    hits.truncate(MAX_RESULTS);
    if hits.is_empty() {
        return WebEvidence {
            query: query.to_string(),
            hits,
            error: Some(
                notes
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| "Nessun risultato web utilizzabile.".into()),
            ),
        };
    }

    fetch_top_pages(&mut hits).await;
    WebEvidence {
        query: query.to_string(),
        hits,
        error: if notes.is_empty() {
            None
        } else {
            Some(notes.join(" "))
        },
    }
}

async fn search_duckduckgo(client: &reqwest::Client, query: &str) -> Result<Vec<SearchHit>, String> {
    let html = post_search(client, SEARCH_URL, query).await?;
    if is_ddg_blocked(&html) {
        return Ok(vec![]);
    }
    let mut hits = parse_ddg_html(&html);
    if hits.is_empty() {
        let lite = client
            .get(SEARCH_LITE_URL)
            .query(&[("q", query), ("kl", "it-it")])
            .send()
            .await
            .map_err(|e| format!("DuckDuckGo lite: {e}"))?
            .text()
            .await
            .map_err(|e| format!("DuckDuckGo lite body: {e}"))?;
        if !is_ddg_blocked(&lite) {
            hits = parse_ddg_html(&lite);
        }
    }
    hits.truncate(MAX_RESULTS);
    Ok(hits)
}

fn is_ddg_blocked(html: &str) -> bool {
    html.contains("anomaly-modal") || html.contains("Unfortunately, bots use DuckDuckGo")
}

async fn post_search(client: &reqwest::Client, url: &str, query: &str) -> Result<String, String> {
    let resp = client
        .post(url)
        .header("Referer", "https://duckduckgo.com/")
        .header("Accept-Language", "it-IT,it;q=0.9,en;q=0.8")
        .form(&[("q", query), ("kl", "it-it")])
        .send()
        .await
        .map_err(|e| format!("DuckDuckGo search failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("DuckDuckGo HTTP {}", resp.status()));
    }
    resp.text()
        .await
        .map_err(|e| format!("DuckDuckGo body: {e}"))
}

async fn ddg_instant_answer(
    client: &reqwest::Client,
    query: &str,
) -> Result<Vec<SearchHit>, String> {
    let resp = client
        .get("https://api.duckduckgo.com/")
        .query(&[
            ("q", query),
            ("format", "json"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
            ("t", "agentos"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parse_ddg_instant(&v))
}

fn parse_ddg_instant(v: &serde_json::Value) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let abstract_text = v
        .get("AbstractText")
        .or_else(|| v.get("Abstract"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    let heading = v.get("Heading").and_then(|x| x.as_str()).unwrap_or("");
    let url = v.get("AbstractURL").and_then(|x| x.as_str()).unwrap_or("");
    if !abstract_text.is_empty() && is_public_http_url(url) {
        hits.push(SearchHit {
            title: if heading.is_empty() {
                "DuckDuckGo Instant Answer".into()
            } else {
                heading.into()
            },
            url: url.into(),
            snippet: truncate_chars(abstract_text, MAX_SNIPPET_CHARS),
            page_text: Some(truncate_chars(abstract_text, MAX_PAGE_CHARS)),
        });
    }
    if let Some(related) = v.get("RelatedTopics").and_then(|t| t.as_array()) {
        for item in related.iter().take(6) {
            collect_related(item, &mut hits);
            if hits.len() >= MAX_RESULTS {
                break;
            }
        }
    }
    hits
}

fn collect_related(item: &serde_json::Value, hits: &mut Vec<SearchHit>) {
    if let Some(topics) = item.get("Topics").and_then(|t| t.as_array()) {
        for t in topics {
            collect_related(t, hits);
        }
        return;
    }
    let url = item.get("FirstURL").and_then(|x| x.as_str()).unwrap_or("");
    let text = item.get("Text").and_then(|x| x.as_str()).unwrap_or("");
    if text.is_empty() || !is_public_http_url(url) {
        return;
    }
    hits.push(SearchHit {
        title: truncate_chars(text.split(" - ").next().unwrap_or(text), 160),
        url: url.into(),
        snippet: truncate_chars(text, MAX_SNIPPET_CHARS),
        page_text: None,
    });
}

async fn wikipedia_search(
    client: &reqwest::Client,
    lang: &str,
    query: &str,
) -> Result<Vec<SearchHit>, String> {
    let url = format!("https://{lang}.wikipedia.org/w/api.php");
    let resp = client
        .get(&url)
        .query(&[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("utf8", "1"),
            ("format", "json"),
            ("srlimit", "4"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parse_wikipedia_search(lang, &v))
}

fn parse_wikipedia_search(lang: &str, v: &serde_json::Value) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let Some(arr) = v.pointer("/query/search").and_then(|s| s.as_array()) else {
        return hits;
    };
    for item in arr {
        let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("");
        if title.is_empty() {
            continue;
        }
        let snippet = strip_tags(html_unescape(
            item.get("snippet").and_then(|s| s.as_str()).unwrap_or(""),
        ));
        let page = format!(
            "https://{lang}.wikipedia.org/wiki/{}",
            title.replace(' ', "_")
        );
        if !is_public_http_url(&page) {
            continue;
        }
        hits.push(SearchHit {
            title: title.into(),
            url: page,
            snippet: truncate_chars(&snippet, MAX_SNIPPET_CHARS),
            page_text: None,
        });
    }
    hits
}

async fn wikidata_search(client: &reqwest::Client, query: &str) -> Result<Vec<SearchHit>, String> {
    let resp = client
        .get("https://www.wikidata.org/w/api.php")
        .query(&[
            ("action", "wbsearchentities"),
            ("search", query),
            ("language", "it"),
            ("uselang", "it"),
            ("limit", "3"),
            ("format", "json"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parse_wikidata(&v))
}

fn parse_wikidata(v: &serde_json::Value) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let Some(arr) = v.get("search").and_then(|s| s.as_array()) else {
        return hits;
    };
    for item in arr {
        let id = item.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let label = item.get("label").and_then(|x| x.as_str()).unwrap_or(id);
        let descr = item
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let url = format!("https://www.wikidata.org/wiki/{id}");
        hits.push(SearchHit {
            title: label.into(),
            url,
            snippet: truncate_chars(descr, MAX_SNIPPET_CHARS),
            page_text: if descr.is_empty() {
                None
            } else {
                Some(format!("{label}: {descr}"))
            },
        });
    }
    hits
}

async fn news_rss(client: &reqwest::Client, query: &str) -> Result<Vec<SearchHit>, String> {
    let resp = client
        .get("https://news.google.com/rss/search")
        .query(&[("q", query), ("hl", "it"), ("gl", "IT"), ("ceid", "IT:it")])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let xml = resp.text().await.map_err(|e| e.to_string())?;
    Ok(parse_rss_items(&xml))
}

fn parse_rss_items(xml: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let re = rss_item_re();
    for cap in re.captures_iter(xml) {
        let block = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let title = strip_cdata(tag_text(block, "title").unwrap_or_default());
        let link = strip_cdata(tag_text(block, "link").unwrap_or_default());
        let descr = strip_cdata(tag_text(block, "description").unwrap_or_default());
        if title.is_empty() || !is_public_http_url(&link) {
            continue;
        }
        // Google News feed title is the query itself on the first item
        if title.contains("Google News") {
            continue;
        }
        hits.push(SearchHit {
            title: truncate_chars(&strip_tags(title), 160),
            url: link,
            snippet: truncate_chars(&html_to_text(&descr), MAX_SNIPPET_CHARS),
            page_text: None,
        });
        if hits.len() >= 3 {
            break;
        }
    }
    hits
}

fn tag_text(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let rest = &block[start..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim().to_string())
}

fn strip_cdata(s: String) -> String {
    s.replace("<![CDATA[", "")
        .replace("]]>", "")
        .trim()
        .to_string()
}

fn merge_hits(dst: &mut Vec<SearchHit>, extra: Vec<SearchHit>) {
    let mut seen: std::collections::HashSet<String> =
        dst.iter().map(|h| h.url.to_ascii_lowercase()).collect();
    for h in extra {
        let key = h.url.to_ascii_lowercase();
        if seen.insert(key) {
            dst.push(h);
        }
    }
}

fn source_rank(url: &str, query: &str) -> u8 {
    let u = url.to_ascii_lowercase();
    let q = query.to_ascii_lowercase();
    let want_profiles =
        q.contains("linkedin") || q.contains("github") || q.contains("profilo");
    if want_profiles {
        if u.contains("linkedin.com") {
            return 0;
        }
        if u.contains("github.com") || u.contains("gitlab.com") {
            return 1;
        }
    }
    if u.contains("wikipedia.org") {
        2
    } else if u.contains("wikidata.org") {
        3
    } else if u.contains("news.google.com") {
        5
    } else {
        4
    }
}

fn skip_page_fetch(url: &str) -> bool {
    let Ok(u) = Url::parse(url) else {
        return true;
    };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    host.contains("linkedin.com")
        || host.contains("facebook.com")
        || host.contains("instagram.com")
        || host == "x.com"
        || host.ends_with(".x.com")
        || host.contains("twitter.com")
        || host.contains("tiktok.com")
}

async fn fetch_top_pages(hits: &mut [SearchHit]) {
    let client = match http_client(PAGE_TIMEOUT) {
        Ok(c) => c,
        Err(_) => return,
    };
    let targets: Vec<(usize, String)> = hits
        .iter()
        .enumerate()
        .filter(|(_, h)| !skip_page_fetch(&h.url) && is_public_http_url(&h.url))
        .take(MAX_PAGES)
        .map(|(i, h)| (i, h.url.clone()))
        .collect();
    let futs: Vec<_> = targets
        .iter()
        .map(|(_, url)| {
            let client = client.clone();
            let url = url.clone();
            async move { fetch_page_text(&client, &url).await }
        })
        .collect();
    let pages = join_all(futs).await;
    for ((idx, _), page) in targets.into_iter().zip(pages.into_iter()) {
        if let Ok(text) = page {
            if !text.is_empty() {
                hits[idx].page_text = Some(text);
            }
        }
    }
}

async fn fetch_page_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    if !is_public_http_url(url) {
        return Err("blocked url".into());
    }
    if let Some(extract) = wikipedia_rest_extract(client, url).await {
        return Ok(extract);
    }
    let resp = client
        .get(url)
        .header("Accept", "text/html,text/plain;q=0.9,*/*;q=0.1")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !ctype.is_empty()
        && !(ctype.contains("text/html")
            || ctype.contains("text/plain")
            || ctype.contains("application/xhtml")
            || ctype.contains("charset="))
    {
        return Err(format!("skip content-type {ctype}"));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let slice = if bytes.len() > MAX_BODY_BYTES {
        &bytes[..MAX_BODY_BYTES]
    } else {
        &bytes
    };
    let html = String::from_utf8_lossy(slice);
    Ok(truncate_chars(&html_to_text(&html), MAX_PAGE_CHARS))
}

async fn wikipedia_rest_extract(client: &reqwest::Client, url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !host.ends_with("wikipedia.org") {
        return None;
    }
    let lang = host.split('.').next()?;
    let title = parsed.path().strip_prefix("/wiki/")?;
    if title.is_empty() || title.contains(':') && !title.contains("%") {
        // skip special namespaces roughly; still allow pages with colons if encoded
        if title.starts_with("Special:")
            || title.starts_with("File:")
            || title.starts_with("Help:")
            || title.starts_with("Wikipedia:")
        {
            return None;
        }
    }
    let api = format!("https://{lang}.wikipedia.org/api/rest_v1/page/summary/{title}");
    let resp = client.get(api).send().await.ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    let extract = v.get("extract")?.as_str()?.trim();
    if extract.is_empty() {
        return None;
    }
    Some(truncate_chars(extract, MAX_PAGE_CHARS))
}

fn http_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(4))
        .build()
        .map_err(|e| e.to_string())
}

pub fn parse_ddg_html(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let re_a = result_a_re();
    for cap in re_a.captures_iter(html) {
        let href = cap
            .get(1)
            .or_else(|| cap.get(3))
            .map(|m| m.as_str())
            .unwrap_or("");
        let title = strip_tags(html_unescape(
            cap.get(2)
                .or_else(|| cap.get(4))
                .map(|m| m.as_str())
                .unwrap_or(""),
        ));
        if title.is_empty() {
            continue;
        }
        let Some(url) = resolve_result_url(href) else {
            continue;
        };
        if !is_public_http_url(&url) || is_noise_host(&url) {
            continue;
        }
        if !seen.insert(url.clone()) {
            continue;
        }
        hits.push(SearchHit {
            title: truncate_chars(&title, 160),
            url,
            snippet: String::new(),
            page_text: None,
        });
        if hits.len() >= MAX_RESULTS * 2 {
            break;
        }
    }

    let snippets = collect_snippets(html);
    for (hit, snip) in hits.iter_mut().zip(snippets.into_iter()) {
        if hit.snippet.is_empty() {
            hit.snippet = truncate_chars(&snip, MAX_SNIPPET_CHARS);
        }
    }
    hits.truncate(MAX_RESULTS);
    hits
}

fn collect_snippets(html: &str) -> Vec<String> {
    let re = snippet_re();
    re.captures_iter(html)
        .filter_map(|c| c.get(1).map(|m| m.as_str()))
        .map(|s| truncate_chars(&strip_tags(html_unescape(s)), MAX_SNIPPET_CHARS))
        .filter(|s| s.chars().count() > 20)
        .collect()
}

fn resolve_result_url(href: &str) -> Option<String> {
    let href = html_unescape(href).replace("&amp;", "&");
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://duckduckgo.com{href}")
    } else {
        href.to_string()
    };
    let url = Url::parse(&absolute).ok()?;
    for (k, v) in url.query_pairs() {
        if k == "uddg" {
            return Url::parse(&v).ok().map(|u| u.to_string()).or(Some(v.into_owned()));
        }
    }
    Some(url.to_string())
}

fn is_public_http_url(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host_l = host.to_ascii_lowercase();
    if host_l == "localhost" || host_l.ends_with(".local") || host_l.ends_with(".localhost") {
        return false;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                !(v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_multicast()
                    || v4.is_unspecified()
                    || v4.octets()[0] == 0)
            }
            IpAddr::V6(v6) => !(v6.is_loopback() || v6.is_multicast() || v6.is_unspecified()),
        };
    }
    true
}

fn is_noise_host(url: &str) -> bool {
    let Ok(u) = Url::parse(url) else {
        return true;
    };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    host == "duckduckgo.com"
        || host.ends_with(".duckduckgo.com")
        || host == "youtube.com" && u.path().starts_with("/ads")
}

pub fn html_to_text(html: &str) -> String {
    let no_blocks = drop_blocks_re().replace_all(html, " ");
    let br = br_re().replace_all(&no_blocks, "\n");
    let blocks = block_end_re().replace_all(&br, "\n");
    let no_tags = tags_re().replace_all(&blocks, " ");
    let unescaped = html_unescape(&no_tags);
    collapse_ws(&unescaped)
}

fn strip_tags(s: String) -> String {
    collapse_ws(&tags_re().replace_all(&s, " "))
}

fn html_unescape(s: &str) -> String {
    let mut out = s
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    // numeric entities &#NNN;
    if out.contains("&#") {
        let re = numeric_ent_re();
        out = re
            .replace_all(&out, |caps: &regex::Captures| {
                caps.get(1)
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            })
            .into_owned();
    }
    out
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

fn truncate_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

fn result_a_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?is)<a\b[^>]*class="[^"]*(?:result__a|result-link)[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>|<a\b[^>]*href="([^"]+)"[^>]*class="[^"]*(?:result__a|result-link)[^"]*"[^>]*>(.*?)</a>"#,
        )
        .expect("result_a regex")
    })
}

fn snippet_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?is)class="[^"]*(?:result__snippet|result-snippet)[^"]*"[^>]*>(.*?)</(?:a|td|span)>"#)
            .expect("snippet regex")
    })
}

fn drop_blocks_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?is)<script[^>]*>.*?</script>|<style[^>]*>.*?</style>|<noscript[^>]*>.*?</noscript>|<svg[^>]*>.*?</svg>|<iframe[^>]*>.*?</iframe>",
        )
        .expect("drop blocks")
    })
}

fn br_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<br\s*/?>").expect("br"))
}

fn block_end_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)</(p|div|h[1-6]|li|tr|section|article)>").expect("block end"))
}

fn tags_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<[^>]+>").expect("tags"))
}

fn rss_item_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<item>(.*?)</item>").expect("rss item"))
}

fn numeric_ent_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"&#(\d+);").expect("numeric ent"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cerca_online_colon() {
        let cmd = parse_command("cerca online: chi è Andrea Scipio").unwrap();
        assert_eq!(
            cmd,
            WebCommand::Query("chi è Andrea Scipio".into())
        );
    }

    #[test]
    fn parse_cerca_online_space() {
        let cmd = parse_command("Cerca sul web ultime notizie su Roma").unwrap();
        assert_eq!(
            cmd,
            WebCommand::Query("ultime notizie su Roma".into())
        );
    }

    #[test]
    fn parse_help_when_empty() {
        assert_eq!(parse_command("cerca online:"), Some(WebCommand::Help));
        assert_eq!(parse_command("cerca online"), Some(WebCommand::Help));
    }

    #[test]
    fn parse_ignores_session_search() {
        assert!(parse_command("cerca: progetto agentos").is_none());
        assert!(parse_command("task: comprare latte").is_none());
    }

    #[test]
    fn strip_chi_e() {
        assert_eq!(
            strip_question_shell("Chi è andrea Scipio?"),
            "andrea Scipio"
        );
    }

    #[test]
    fn followup_detects_php_and_links() {
        assert!(is_web_followup(
            "Si sto cercando quello che è uno sviluppatore php"
        ));
        assert!(is_web_followup(
            "Ok però dammi il link dei suoi profili così controllo io"
        ));
        assert!(is_profile_request("dammi il link dei suoi profili"));
        assert!(!is_web_followup("grazie"));
        assert!(!is_web_followup("task: comprare latte"));
        assert!(!is_web_followup("salva un task: comprare latte"));
        assert!(!is_web_followup("ricorda di comprare il latte"));
    }

    #[test]
    fn compose_followup_adds_profiles() {
        let thread = WebThread {
            session_id: "s".into(),
            subject: "Andrea Scipio".into(),
            constraints: "sviluppatore php".into(),
        };
        let q = compose_followup_query(&thread, "dammi i link dei profili");
        assert!(q.contains("Andrea Scipio"));
        assert!(q.to_lowercase().contains("linkedin"));
        assert!(q.to_lowercase().contains("github"));
    }

    #[test]
    fn html_to_text_strips_script() {
        let t = html_to_text("<html><script>alert(1)</script><p>Ciao &amp; mondo</p></html>");
        assert!(t.contains("Ciao & mondo"));
        assert!(!t.contains("alert"));
    }

    #[test]
    fn parse_ddg_result_a() {
        let html = r#"
            <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fit.wikipedia.org%2Fwiki%2FAndrea_Scipio">Andrea Scipio</a>
            <a class="result__snippet" href="snip">Regista e conduttore italiano.</a>
        "#;
        let hits = parse_ddg_html(html);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Andrea Scipio");
        assert!(hits[0].url.contains("wikipedia.org"));
        assert!(hits[0].snippet.contains("Regista"));
    }

    #[test]
    fn blocks_localhost() {
        assert!(!is_public_http_url("http://127.0.0.1/secret"));
        assert!(!is_public_http_url("http://localhost/x"));
        assert!(is_public_http_url("https://it.wikipedia.org/wiki/Test"));
    }

    #[test]
    fn detects_ddg_captcha() {
        assert!(is_ddg_blocked(r#"<div class="anomaly-modal__title">hmm</div>"#));
        assert!(!is_ddg_blocked(r#"<a class="result__a" href="https://example.com">x</a>"#));
    }

    #[test]
    fn parse_wikipedia_json() {
        let v = serde_json::json!({
            "query": {"search": [
                {"title": "Albert Einstein", "snippet": "fisico <span>tedesco</span>"}
            ]}
        });
        let hits = parse_wikipedia_search("it", &v);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].url.ends_with("/wiki/Albert_Einstein"));
        assert!(hits[0].snippet.contains("fisico"));
        assert!(!hits[0].snippet.contains("<span>"));
    }

    #[test]
    fn parse_rss_skips_feed_title() {
        let xml = r#"
        <rss><channel>
          <item><title>Google News</title><link>https://news.google.com/</link><description>x</description></item>
          <item><title>Ultim'ora su Roma</title><link>https://www.repubblica.it/n1</link><description>oggi in città</description></item>
        </channel></rss>
        "#;
        let hits = parse_rss_items(xml);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Ultim'ora su Roma");
    }

    #[test]
    fn parse_instant_answer() {
        let v = serde_json::json!({
            "Heading": "Albert Einstein",
            "AbstractText": "Fisico teorico.",
            "AbstractURL": "https://it.wikipedia.org/wiki/Albert_Einstein",
            "RelatedTopics": []
        });
        let hits = parse_ddg_instant(&v);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Albert Einstein");
    }
}

// Live check (needs network). Not part of the default suite.
#[cfg(test)]
mod live {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn search_einstein_has_sources() {
        let ev = search_and_fetch("Albert Einstein").await;
        assert!(
            !ev.hits.is_empty(),
            "expected web hits, error={:?}",
            ev.error
        );
        assert!(ev.hits.iter().any(|h| h.url.contains("wikipedia") || !h.snippet.is_empty()));
    }
}
