use crate::db::{now, Db};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use regex::Regex;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub color: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: i64,
    pub deadline: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id: i64,
    pub r#type: String,
    pub schedule: String,
    pub message: String,
    pub enabled: bool,
    pub fired: bool,
    pub created_at: String,
    #[serde(default)]
    pub task_id: Option<i64>,
    #[serde(default = "default_recurrence")]
    pub recurrence: String,
}

fn default_recurrence() -> String {
    "none".into()
}

#[derive(Debug, Clone)]
pub struct ParsedReminder {
    pub schedule_rfc3339: String,
    pub message: String,
    pub human: String,
    pub task_id: Option<i64>,
    pub recurrence: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelSpec {
    pub kind: String,
    pub name: String,
}

/// Relative delay units, including seconds (for tests like “tra 90 secondi”).
const REL_UNITS: &str =
    r"(secondi|secondo|sec|seconds?|minuti|minuto|min|minutes?|ore|ora|hours?|hour|h)";
const REMINDER_VERB: &str =
    r"(?:ricordarmi|ricordami|ricordare|ricorderesti|avvisami|avvisarmi|notificami|promemoria|reminder)";

fn re_tra_after_verb() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?i){REMINDER_VERB}\b(?:\s+\w+){{0,10}}?\s+(?:tra|in)\s+(\d+)\s*{REL_UNITS}\b\s*(?:di|che|per|:)?\s*(.+)$"
        ))
        .expect("re_tra_after_verb")
    })
}

fn re_tra_before_verb() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?i)(?:tra|in)\s+(\d+)\s*{REL_UNITS}\b\s+(?:ricordami|avvisami|notificami)\s*(?:di|che|per|:)?\s*(.+)$"
        ))
        .expect("re_tra_before_verb")
    })
}

fn re_msg_then_tra() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?i){REMINDER_VERB}\s+(?:di|che|per|:)\s+(.+?)\s+(?:tra|in)\s+(\d+)\s*{REL_UNITS}\s*$"
        ))
        .expect("re_msg_then_tra")
    })
}

fn re_oggi_domani() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?i){REMINDER_VERB}\b(?:\s+\w+){{0,8}}?\s+(oggi|domani)\s+alle\s+(\d{{1,2}})(?::(\d{{2}}))?\s*(?:di|che|per|:)?\s*(.+)$"
        ))
        .expect("re_oggi_domani")
    })
}

fn re_data() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?i){REMINDER_VERB}\b(?:\s+\w+){{0,8}}?\s+(?:il\s+)?(\d{{4}}-\d{{2}}-\d{{2}})\s+(?:alle\s+)?(\d{{1,2}})(?::(\d{{2}}))?\s*(?:di|che|per|:)?\s*(.+)$"
        ))
        .expect("re_data")
    })
}

fn duration_from_unit(n: i64, unit: &str) -> Result<Duration, String> {
    if n <= 0 {
        return Err("il ritardo deve essere positivo".into());
    }
    let u = unit.to_lowercase();
    if u.starts_with("sec") {
        Ok(Duration::seconds(n))
    } else if u.starts_with("min") {
        Ok(Duration::minutes(n))
    } else {
        Ok(Duration::hours(n))
    }
}

fn clean_reminder_message(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    loop {
        let lower = s.to_lowercase();
        let prefix = ["di ", "che ", "per ", ": "]
            .iter()
            .find(|p| lower.starts_with(*p));
        match prefix {
            Some(p) => s = s[p.len()..].trim().to_string(),
            None => break,
        }
    }
    while matches!(s.chars().last(), Some('.' | '!' | '?' | ',')) {
        s.pop();
        s = s.trim_end().to_string();
    }
    s
}

fn relative_reminder(n: i64, unit: &str, message: &str) -> Result<ParsedReminder, String> {
    let message = clean_reminder_message(message);
    if message.is_empty() {
        return Err("Messaggio promemoria vuoto".into());
    }
    let dur = duration_from_unit(n, unit)?;
    let when = Local::now() + dur;
    Ok(ParsedReminder {
        human: format!("tra {n} {unit}"),
        schedule_rfc3339: when.to_rfc3339(),
        message,
        task_id: None,
        recurrence: "none".into(),
    })
}

/// True when the user is asking to schedule a reminder (even in conversational Italian).
pub fn looks_like_reminder_intent(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    if lower.starts_with("promemoria:") || lower.starts_with("reminder:") {
        return true;
    }
    let verb = lower.contains("ricordam")
        || lower.contains("ricordar")
        || lower.contains("ricorder")
        || lower.contains("promemoria")
        || lower.contains("reminder")
        || lower.contains("avvisam")
        || lower.contains("avvisarm")
        || lower.contains("notificam");
    if !verb {
        return false;
    }
    lower.contains("tra ")
        || lower.contains(" in ")
        || lower.starts_with("in ")
        || lower.contains("oggi")
        || lower.contains("domani")
        || lower.contains("alle ")
        || re_has_date().is_match(&lower)
}

fn re_has_date() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d{4}-\d{2}-\d{2}").expect("date-re"))
}

/// Normalize a schedule string to local RFC3339.
pub fn normalize_schedule(schedule: &str) -> Result<String, String> {
    let s = schedule.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Local).to_rfc3339());
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        let local = ndt
            .and_local_timezone(Local)
            .single()
            .ok_or_else(|| "orario ambiguo (DST)".to_string())?;
        return Ok(local.to_rfc3339());
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        let local = ndt
            .and_local_timezone(Local)
            .single()
            .ok_or_else(|| "orario ambiguo (DST)".to_string())?;
        return Ok(local.to_rfc3339());
    }
    Err("schedule must be RFC3339 or 'YYYY-MM-DD HH:MM'".into())
}

fn local_at(date: NaiveDate, hour: u32, minute: u32) -> Result<DateTime<Local>, String> {
    let time = NaiveTime::from_hms_opt(hour, minute, 0).ok_or("orario non valido")?;
    NaiveDateTime::new(date, time)
        .and_local_timezone(Local)
        .single()
        .ok_or_else(|| "orario ambiguo (DST)".to_string())
}

/// Parse Italian/English natural-language reminder commands.
pub fn parse_reminder_nl(raw: &str) -> Result<ParsedReminder, String> {
    let text = raw.trim();

    // Explicit: promemoria: SCHEDULE | message  OR  reminder: ...
    let lower = text.to_lowercase();
    if lower.starts_with("promemoria:") || lower.starts_with("reminder:") {
        let rest = text.split_once(':').map(|(_, r)| r.trim()).unwrap_or("");
        let (schedule, message) = rest
            .split_once('|')
            .map(|(a, b)| (a.trim(), b.trim()))
            .unwrap_or((rest, "Promemoria"));
        if schedule.is_empty() {
            return Err("Indica data/ora. Es: promemoria: 2026-08-02 09:00 | messaggio".into());
        }
        let rfc = normalize_schedule(schedule)?;
        return Ok(ParsedReminder {
            human: format!("{schedule} (locale)"),
            schedule_rfc3339: rfc,
            message: if message.is_empty() {
                "Promemoria".into()
            } else {
                message.into()
            },
            task_id: None,
            recurrence: "none".into(),
        });
    }

    if let Some(c) = re_tra_after_verb().captures(text) {
        let n: i64 = c[1].parse().map_err(|_| "numero non valido")?;
        return relative_reminder(n, &c[2], &c[3]);
    }
    if let Some(c) = re_tra_before_verb().captures(text) {
        let n: i64 = c[1].parse().map_err(|_| "numero non valido")?;
        return relative_reminder(n, &c[2], &c[3]);
    }
    if let Some(c) = re_msg_then_tra().captures(text) {
        let n: i64 = c[2].parse().map_err(|_| "numero non valido")?;
        return relative_reminder(n, &c[3], &c[1]);
    }

    if let Some(c) = re_oggi_domani().captures(text) {
        let day = c[1].to_lowercase();
        let hour: u32 = c[2].parse().map_err(|_| "ora non valida")?;
        let minute: u32 = c
            .get(3)
            .map(|m| m.as_str().parse().unwrap_or(0))
            .unwrap_or(0);
        let message = clean_reminder_message(&c[4]);
        if message.is_empty() {
            return Err("Messaggio promemoria vuoto".into());
        }
        let today = Local::now().date_naive();
        let date = if day == "domani" {
            today + Duration::days(1)
        } else {
            today
        };
        let when = local_at(date, hour, minute)?;
        if day == "oggi" && when <= Local::now() {
            return Err("L'orario di oggi è già passato. Usa 'domani' o un orario futuro.".into());
        }
        return Ok(ParsedReminder {
            human: format!("{day} alle {hour:02}:{minute:02}"),
            schedule_rfc3339: when.to_rfc3339(),
            message,
            task_id: None,
            recurrence: "none".into(),
        });
    }

    if let Some(c) = re_data().captures(text) {
        let date = NaiveDate::parse_from_str(&c[1], "%Y-%m-%d")
            .map_err(|_| "data non valida (YYYY-MM-DD)")?;
        let hour: u32 = c[2].parse().map_err(|_| "ora non valida")?;
        let minute: u32 = c
            .get(3)
            .map(|m| m.as_str().parse().unwrap_or(0))
            .unwrap_or(0);
        let message = clean_reminder_message(&c[4]);
        if message.is_empty() {
            return Err("Messaggio promemoria vuoto".into());
        }
        let when = local_at(date, hour, minute)?;
        return Ok(ParsedReminder {
            human: format!("{} alle {hour:02}:{minute:02}", c[1].to_string()),
            schedule_rfc3339: when.to_rfc3339(),
            message,
            task_id: None,
            recurrence: "none".into(),
        });
    }

    Err(
        "Formati supportati:\n\
         • ricordami tra 90 secondi di …\n\
         • ricordami tra 5 minuti di …\n\
         • ricordami di … tra 2 minuti\n\
         • ricordami domani alle 9 di …\n\
         • ricordami oggi alle 15:30 di …\n\
         • ricordami 2026-08-02 alle 10:00 di …\n\
         • promemoria: 2026-08-02 09:00 | messaggio"
            .into(),
    )
}

fn parse_when(schedule: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(schedule) {
        return Some(dt.with_timezone(&Utc));
    }
    // Legacy naive timestamps were incorrectly treated as UTC; interpret as Local.
    if let Ok(ndt) = NaiveDateTime::parse_from_str(schedule, "%Y-%m-%d %H:%M") {
        return ndt
            .and_local_timezone(Local)
            .single()
            .map(|d| d.with_timezone(&Utc));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(schedule, "%Y-%m-%d %H:%M:%S") {
        return ndt
            .and_local_timezone(Local)
            .single()
            .map(|d| d.with_timezone(&Utc));
    }
    None
}

/// Result of parsing a natural-language “create task” request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskCreate {
    Ready {
        title: String,
        priority: i64,
        deadline: Option<String>,
        labels: Vec<LabelSpec>,
    },
    NeedTitle,
    NotATask,
}

pub fn normalize_task_status(status: &str) -> Result<&'static str, String> {
    let s = status.trim().to_lowercase();
    match s.as_str() {
        "open" | "todo" | "aperto" => Ok("open"),
        "doing" | "in_progress" | "in-progress" | "progress" | "corso" | "in corso" => Ok("doing"),
        "done" | "completato" | "completa" | "fatto" => Ok("done"),
        other => Err(format!(
            "status non valido «{other}» (usa open, doing, done)"
        )),
    }
}

pub fn normalize_label_kind(kind: &str) -> Result<&'static str, String> {
    match kind.trim().to_lowercase().as_str() {
        "project" | "progetto" | "proj" => Ok("project"),
        "client" | "cliente" | "cli" => Ok("client"),
        "area" | "contesto" | "context" => Ok("area"),
        "tag" | "etichetta" | "label" => Ok("tag"),
        other => Err(format!(
            "tipo etichetta non valido «{other}» (project|client|area|tag)"
        )),
    }
}

fn label_palette() -> &'static [&'static str] {
    &[
        "#5b8def", "#3db89a", "#e5a84a", "#e86b8a", "#9b7bff", "#4ecdc4",
        "#f0734a", "#6bcb77", "#c77dff", "#4d96ff", "#ff6b6b", "#20c997",
        "#fdae4b", "#845ef7", "#22b8cf", "#f06595", "#51cf66", "#748ffc",
        "#ff922b", "#cc5de8", "#15aabf", "#fa5252", "#94d82d", "#339af0",
    ]
}

fn color_from_seed(seed: &str) -> &'static str {
    let mut h: u32 = 2166136261;
    for b in seed.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(16777619);
    }
    let palette = label_palette();
    palette[(h as usize) % palette.len()]
}

fn is_generic_kind_color(color: &str) -> bool {
    matches!(
        color.trim().to_lowercase().as_str(),
        "#5b8def" | "#3db89a" | "#e5a84a" | "#9b7bff"
    )
}

/// Prefer stored color; if still a generic kind default, derive a distinct hue from name+id.
pub fn effective_label_color(label: &Label) -> String {
    if !is_generic_kind_color(&label.color) {
        return label.color.clone();
    }
    color_from_seed(&format!("{}:{}:{}", label.kind, label.name, label.id)).to_string()
}

fn parse_deadline_token(raw: &str) -> Option<String> {
    let s = raw.trim().to_lowercase();
    let today = Local::now().date_naive();
    let date = if s == "oggi" || s == "today" {
        today
    } else if s == "domani" || s == "tomorrow" {
        today + Duration::days(1)
    } else if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        d
    } else {
        return None;
    };
    local_at(date, 18, 0)
        .ok()
        .map(|dt| dt.to_rfc3339())
}

/// Strip trailing deadline hints from a task title: "report entro domani", "x per 2026-08-30".
fn extract_deadline_from_title(title: &str) -> (String, Option<String>) {
    let lower = title.to_lowercase();
    for marker in [" entro ", " scadenza ", " due "] {
        if let Some(idx) = lower.rfind(marker) {
            let after = title[idx + marker.len()..].trim();
            let token = after.split_whitespace().next().unwrap_or("");
            if token.chars().count() <= 12 {
                if let Some(dl) = parse_deadline_token(token) {
                    let mut cleaned = title[..idx].trim().to_string();
                    while matches!(cleaned.chars().last(), Some(':' | ',' | '-')) {
                        cleaned.pop();
                        cleaned = cleaned.trim_end().to_string();
                    }
                    if !cleaned.is_empty() {
                        return (cleaned, Some(dl));
                    }
                }
            }
        }
    }
    // "per YYYY-MM-DD" / "per domani" only when token is clearly a date
    if let Some(idx) = lower.rfind(" per ") {
        let after = title[idx + 5..].trim();
        let token = after.split_whitespace().next().unwrap_or("");
        if parse_deadline_token(token).is_some() && token.chars().count() <= 12 {
            if let Some(dl) = parse_deadline_token(token) {
                let cleaned = title[..idx].trim().to_string();
                if !cleaned.is_empty() {
                    return (cleaned, Some(dl));
                }
            }
        }
    }
    (title.trim().to_string(), None)
}

fn re_hash_tag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:^|\s)#([\w\-àèéìòù]+)").expect("hash"))
}

fn re_kind_label() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:^|\s)(progetto|project|cliente|client|area|contesto|context)[:\s]+([^\s#]+(?:\s+[^\s#]+){0,2})",
        )
        .expect("kind-label")
    })
}

fn re_add_to_client() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:aggiungi|salva|crea|metti).{0,24}?(?:al|per|sul)?\s*(?:cliente|client)\s+([^\s:]+(?:\s+[^\s:]+){0,2})\s*:\s*(.+)$",
        )
        .expect("add-client")
    })
}

fn re_add_to_project() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:aggiungi|salva|crea|metti).{0,24}?(?:al|per|sul)?\s*(?:progetto|project)\s+([^\s:]+(?:\s+[^\s:]+){0,2})\s*:\s*(.+)$",
        )
        .expect("add-project")
    })
}

/// Pull #tags and progetto/cliente/area phrases out of a title.
pub fn extract_label_specs(title: &str) -> (String, Vec<LabelSpec>) {
    let mut specs: Vec<LabelSpec> = Vec::new();
    let mut s = title.to_string();

    for cap in re_kind_label().captures_iter(&s.clone()) {
        let kind_raw = cap.get(1).map(|m| m.as_str()).unwrap_or("tag");
        let name = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        if let Ok(kind) = normalize_label_kind(kind_raw) {
            specs.push(LabelSpec {
                kind: kind.to_string(),
                name: name.to_string(),
            });
            if let Some(m) = cap.get(0) {
                s = s.replacen(m.as_str(), " ", 1);
            }
        }
    }

    for cap in re_hash_tag().captures_iter(&s.clone()) {
        let name = cap.get(1).map(|m| m.as_str()).unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        specs.push(LabelSpec {
            kind: "tag".into(),
            name: name.to_string(),
        });
        if let Some(m) = cap.get(0) {
            s = s.replacen(m.as_str(), " ", 1);
        }
    }

    let cleaned = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (cleaned, specs)
}

fn is_task_list_or_mutate(lower: &str) -> bool {
    let t = lower.trim();
    t == "lista task"
        || t == "lista tasks"
        || t == "i miei task"
        || t == "todo list"
        || t == "cosa ho da fare"
        || t.starts_with("lista task")
        || t.starts_with("completa task ")
        || t.starts_with("fatto task ")
        || t.starts_with("completa #")
        || t.starts_with("done task ")
        || t.starts_with("elimina task ")
        || t.starts_with("delete task ")
}

fn task_priority(lower: &str) -> i64 {
    if lower.contains("urgente")
        || lower.contains("priorità alta")
        || lower.contains("priorita alta")
        || lower.contains("alta priorità")
        || lower.contains("alta priorita")
        || lower.contains("high priority")
        || (lower.contains(" alta") && lower.contains("prior"))
    {
        1
    } else if lower.contains("bassa") || lower.contains("low priority") {
        3
    } else {
        2
    }
}

fn clean_task_title(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    while matches!(s.chars().last(), Some('?' | '!' | '.' | ',' | ':')) {
        s.pop();
        s = s.trim_end().to_string();
    }
    loop {
        let lower = s.to_lowercase();
        let prefix = [
            "urgente ",
            "urgente:",
            "priorità alta ",
            "priorita alta ",
            "alta priorità ",
            "alta priorita ",
            "high priority ",
            "priorità bassa ",
            "priorita bassa ",
            "bassa priorità ",
            "bassa priorita ",
            "low priority ",
            "chiamato ",
            "chiamata ",
            "intitolato ",
            "intitolata ",
            "per ",
            "di ",
            ": ",
        ]
        .iter()
        .find(|p| lower.starts_with(*p));
        match prefix {
            Some(p) => s = s[p.len()..].trim().to_string(),
            None => break,
        }
    }
    s
}

fn re_task_create() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:^|\b)(?:puoi\s+|mi\s+puoi\s+|per\s+favore\s+|per\s+piacere\s+|please\s+|fammi\s+|mi\s+)?(?:salva(?:re|i)?|crea(?:re)?|aggiung(?:i|ere)|inserisc(?:i|ere)|mett(?:i|ere)|annota(?:re)?|registra(?:re)?|save|add|create)\b.{0,48}?\b(?:task|todo|to-do|attivit[aà])\b\s*(?:chiamat[oa]|intitolat[oa]|per|di|:)?\s*(.*)$",
        )
        .expect("re_task_create")
    })
}

fn re_ricorda_di_task() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(?:puoi\s+|mi\s+puoi\s+|per\s+favore\s+)?ricorda(?:mi)?\s+di\s+(.+)$")
            .expect("re_ricorda_di_task")
    })
}

fn re_remember_to_task() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(?:please\s+|can you\s+)?remember\s+to\s+(.+)$")
            .expect("re_remember_to_task")
    })
}

fn labeled_task_title(raw: &str) -> Option<String> {
    let lower = raw.to_lowercase();
    for label in ["task:", "todo:", "to-do:", "attività:", "attivita:"] {
        if let Some(rest) = lower.find(label) {
            let orig = raw.get(rest + label.len()..)?.trim();
            if !orig.is_empty() {
                return Some(orig.to_string());
            }
            return Some(String::new());
        }
    }
    None
}

/// True when the user is asking to create a to-do (not list/complete, not a timed reminder).
pub fn looks_like_task_create(raw: &str) -> bool {
    !matches!(parse_task_create(raw), TaskCreate::NotATask)
}

/// Parse conversational Italian/English into a task title + priority.
pub fn parse_task_create(raw: &str) -> TaskCreate {
    let text = raw.trim();
    if text.is_empty() {
        return TaskCreate::NotATask;
    }
    let lower = text.to_lowercase();
    if is_task_list_or_mutate(&lower) || looks_like_reminder_intent(text) {
        return TaskCreate::NotATask;
    }

    let priority = task_priority(&lower);

    let ready = |raw_title: String, mut extra: Vec<LabelSpec>| {
        let cleaned = clean_task_title(&raw_title);
        if cleaned.is_empty() {
            return TaskCreate::NeedTitle;
        }
        let (no_labels, mut labels) = extract_label_specs(&cleaned);
        labels.append(&mut extra);
        let (title, deadline) = extract_deadline_from_title(&no_labels);
        if title.is_empty() {
            return TaskCreate::NeedTitle;
        }
        TaskCreate::Ready {
            title,
            priority,
            deadline,
            labels,
        }
    };

    if let Some(c) = re_add_to_client().captures(text) {
        let client = c[1].trim().to_string();
        let title = c[2].trim().to_string();
        return ready(
            title,
            vec![LabelSpec {
                kind: "client".into(),
                name: client,
            }],
        );
    }
    if let Some(c) = re_add_to_project().captures(text) {
        let project = c[1].trim().to_string();
        let title = c[2].trim().to_string();
        return ready(
            title,
            vec![LabelSpec {
                kind: "project".into(),
                name: project,
            }],
        );
    }

    if let Some(title) = labeled_task_title(text) {
        return ready(title, vec![]);
    }

    if let Some(c) = re_task_create().captures(text) {
        return ready(
            c.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
            vec![],
        );
    }

    if let Some(c) = re_ricorda_di_task().captures(text) {
        let title = clean_task_title(&c[1]);
        if !title.is_empty() {
            return ready(title, vec![]);
        }
    }
    if let Some(c) = re_remember_to_task().captures(text) {
        let title = clean_task_title(&c[1]);
        if !title.is_empty() {
            return ready(title, vec![]);
        }
    }

    TaskCreate::NotATask
}

fn map_task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        status: row.get(3)?,
        priority: row.get(4)?,
        deadline: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        labels: Vec::new(),
    })
}

fn attach_labels(conn: &rusqlite::Connection, tasks: &mut [Task]) -> Result<(), String> {
    if tasks.is_empty() {
        return Ok(());
    }
    let mut stmt = conn
        .prepare(
            "SELECT tl.task_id, l.id, l.name, l.kind, l.color, l.created_at
             FROM task_labels tl
             JOIN labels l ON l.id = tl.label_id
             ORDER BY l.kind, l.name",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                Label {
                    id: row.get(1)?,
                    name: row.get(2)?,
                    kind: row.get(3)?,
                    color: row.get(4)?,
                    created_at: row.get(5)?,
                },
            ))
        })
        .map_err(|e| e.to_string())?;
    use std::collections::HashMap;
    let mut map: HashMap<i64, Vec<Label>> = HashMap::new();
    for r in rows {
        let (tid, mut lab) = r.map_err(|e| e.to_string())?;
        lab.color = effective_label_color(&lab);
        map.entry(tid).or_default().push(lab);
    }
    for t in tasks.iter_mut() {
        if let Some(labs) = map.remove(&t.id) {
            t.labels = labs;
        }
    }
    Ok(())
}

pub fn list_tasks(db: &Db) -> Result<Vec<Task>, String> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, status, priority, deadline, created_at, updated_at
             FROM tasks ORDER BY
               CASE status WHEN 'doing' THEN 0 WHEN 'open' THEN 1 WHEN 'done' THEN 2 ELSE 3 END,
               priority,
               CASE WHEN deadline IS NULL OR deadline = '' THEN 1 ELSE 0 END,
               deadline ASC,
               id DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], map_task_row)
        .map_err(|e| e.to_string())?;
    let mut tasks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    attach_labels(&conn, &mut tasks)?;
    Ok(tasks)
}

pub fn list_labels(db: &Db) -> Result<Vec<Label>, String> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, kind, color, created_at FROM labels
             ORDER BY kind, name COLLATE NOCASE",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Label {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                color: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
        .map(|mut labs| {
            for lab in &mut labs {
                lab.color = effective_label_color(lab);
            }
            labs
        })
}

pub fn ensure_label(db: &Db, name: &str, kind: &str) -> Result<i64, String> {
    let kind = normalize_label_kind(kind)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("nome etichetta vuoto".into());
    }
    let conn = db.lock()?;
    if let Ok(id) = conn.query_row(
        "SELECT id FROM labels WHERE name = ?1 COLLATE NOCASE AND kind = ?2",
        params![name, kind],
        |row| row.get::<_, i64>(0),
    ) {
        return Ok(id);
    }
    let color = color_from_seed(&format!("{kind}:{name}"));
    conn.execute(
        "INSERT INTO labels (name, kind, color, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![name, kind, color, now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn create_label(db: &Db, name: &str, kind: &str, color: Option<&str>) -> Result<i64, String> {
    let kind = normalize_label_kind(kind)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("nome etichetta vuoto".into());
    }
    let color = color
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .map(|c| c.to_string())
        .unwrap_or_else(|| color_from_seed(&format!("{kind}:{name}")).to_string());
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO labels (name, kind, color, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![name, kind, color, now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn update_label_color(db: &Db, id: i64, color: &str) -> Result<(), String> {
    let color = color.trim();
    if color.is_empty() || !color.starts_with('#') || !(color.len() == 4 || color.len() == 7) {
        return Err("colore non valido (es. #5b8def)".into());
    }
    let conn = db.lock()?;
    let n = conn
        .execute(
            "UPDATE labels SET color = ?1 WHERE id = ?2",
            params![color, id],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("etichetta #{id} non trovata"));
    }
    Ok(())
}

/// Recolor labels still on generic kind defaults so long lists look distinct.
pub fn recolor_generic_labels(db: &Db) -> Result<usize, String> {
    let labs = list_labels(db)?;
    let mut n = 0usize;
    for lab in labs {
        if !is_generic_kind_color(&lab.color) {
            continue;
        }
        let next = color_from_seed(&format!("{}:{}:{}", lab.kind, lab.name, lab.id));
        if next.eq_ignore_ascii_case(lab.color.trim()) {
            continue;
        }
        update_label_color(db, lab.id, next)?;
        n += 1;
    }
    Ok(n)
}

pub fn delete_label(db: &Db, id: i64) -> Result<(), String> {
    let conn = db.lock()?;
    let n = conn
        .execute("DELETE FROM labels WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("etichetta #{id} non trovata"));
    }
    Ok(())
}

pub fn set_task_labels(db: &Db, task_id: i64, label_ids: &[i64]) -> Result<(), String> {
    let conn = db.lock()?;
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        return Err(format!("task #{task_id} non trovato"));
    }
    conn.execute(
        "DELETE FROM task_labels WHERE task_id = ?1",
        params![task_id],
    )
    .map_err(|e| e.to_string())?;
    for lid in label_ids {
        conn.execute(
            "INSERT OR IGNORE INTO task_labels (task_id, label_id) VALUES (?1, ?2)",
            params![task_id, lid],
        )
        .map_err(|e| e.to_string())?;
    }
    conn.execute(
        "UPDATE tasks SET updated_at = ?1 WHERE id = ?2",
        params![now(), task_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn apply_label_specs(db: &Db, task_id: i64, specs: &[LabelSpec]) -> Result<(), String> {
    if specs.is_empty() {
        return Ok(());
    }
    let mut ids = Vec::new();
    for spec in specs {
        ids.push(ensure_label(db, &spec.name, &spec.kind)?);
    }
    // Merge with existing
    let existing = list_tasks(db)?
        .into_iter()
        .find(|t| t.id == task_id)
        .map(|t| t.labels.into_iter().map(|l| l.id).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut merged = existing;
    for id in ids {
        if !merged.contains(&id) {
            merged.push(id);
        }
    }
    set_task_labels(db, task_id, &merged)
}

pub fn create_task(
    db: &Db,
    title: &str,
    description: &str,
    priority: i64,
    deadline: Option<&str>,
    label_ids: &[i64],
    label_specs: &[LabelSpec],
) -> Result<i64, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("titolo task vuoto".into());
    }
    let priority = priority.clamp(1, 3);
    let deadline_norm = match deadline {
        Some(d) if !d.trim().is_empty() => Some(normalize_schedule(d.trim())?),
        _ => None,
    };
    let id = {
        let conn = db.lock()?;
        let ts = now();
        conn.execute(
            "INSERT INTO tasks (title, description, status, priority, deadline, created_at, updated_at)
             VALUES (?1, ?2, 'open', ?3, ?4, ?5, ?6)",
            params![title, description, priority, deadline_norm, ts, ts],
        )
        .map_err(|e| e.to_string())?;
        conn.last_insert_rowid()
    };
    if !label_ids.is_empty() {
        set_task_labels(db, id, label_ids)?;
    }
    if !label_specs.is_empty() {
        apply_label_specs(db, id, label_specs)?;
    }
    Ok(id)
}

pub fn update_task(
    db: &Db,
    id: i64,
    title: Option<&str>,
    description: Option<&str>,
    priority: Option<i64>,
    deadline: Option<&str>,
    touch_deadline: bool,
    status: Option<&str>,
) -> Result<(), String> {
    let mut task = list_tasks(db)?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| format!("task #{id} non trovato"))?;

    if let Some(t) = title {
        let t = t.trim();
        if t.is_empty() {
            return Err("titolo task vuoto".into());
        }
        task.title = t.to_string();
    }
    if let Some(d) = description {
        task.description = d.to_string();
    }
    if let Some(p) = priority {
        task.priority = p.clamp(1, 3);
    }
    if touch_deadline {
        task.deadline = match deadline {
            Some(d) if !d.trim().is_empty() => Some(normalize_schedule(d.trim())?),
            _ => None,
        };
    }
    if let Some(s) = status {
        task.status = normalize_task_status(s)?.to_string();
    }

    let conn = db.lock()?;
    let n = conn
        .execute(
            "UPDATE tasks SET title = ?1, description = ?2, status = ?3, priority = ?4,
             deadline = ?5, updated_at = ?6 WHERE id = ?7",
            params![
                task.title,
                task.description,
                task.status,
                task.priority,
                task.deadline,
                now(),
                id
            ],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("task #{id} non trovato"));
    }
    Ok(())
}

pub fn set_task_status(db: &Db, id: i64, status: &str) -> Result<(), String> {
    let status = normalize_task_status(status)?;
    let conn = db.lock()?;
    let n = conn
        .execute(
            "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now(), id],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("task #{id} non trovato"));
    }
    Ok(())
}

pub fn delete_task(db: &Db, id: i64) -> Result<(), String> {
    let conn = db.lock()?;
    let n = conn
        .execute("DELETE FROM tasks WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("task #{id} non trovato"));
    }
    Ok(())
}

pub fn render_task_list(db: &Db) -> Result<String, String> {
    let tasks = list_tasks(db)?;
    let open: Vec<_> = tasks
        .iter()
        .filter(|t| t.status == "open" || t.status == "doing")
        .collect();
    if open.is_empty() {
        return Ok("Nessun task aperto. Aggiungine uno con `task: …`.".into());
    }
    let body = open
        .iter()
        .map(|t| {
            let prio = match t.priority {
                1 => "alta",
                3 => "bassa",
                _ => "media",
            };
            let st = if t.status == "doing" { " · in corso" } else { "" };
            let due = t
                .deadline
                .as_deref()
                .map(|d| format!(" · entro {}", format_schedule_human(d)))
                .unwrap_or_default();
            let labs = if t.labels.is_empty() {
                String::new()
            } else {
                let body = t
                    .labels
                    .iter()
                    .map(|l| format!("{}:{}", l.kind, l.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(" · [{body}]")
            };
            format!(
                "#{id} [{prio}]{st}{due}{labs} {title}",
                id = t.id,
                title = t.title
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("Task aperti ({n}):\n{body}", n = open.len()))
}

/// Completed tasks updated within the last `days` (default 30). Kept for silent archive recall.
pub fn list_completed_tasks(db: &Db, days: i64) -> Result<Vec<Task>, String> {
    let days = days.clamp(1, 365);
    let cutoff = (Local::now() - Duration::days(days)).to_rfc3339();
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, status, priority, deadline, created_at, updated_at
             FROM tasks
             WHERE status = 'done' AND updated_at >= ?1
             ORDER BY updated_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![cutoff], map_task_row)
        .map_err(|e| e.to_string())?;
    let mut tasks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    attach_labels(&conn, &mut tasks)?;
    Ok(tasks)
}

pub fn render_completed_tasks(db: &Db, days: i64) -> Result<String, String> {
    let done = list_completed_tasks(db, days)?;
    if done.is_empty() {
        return Ok(format!(
            "Nessun task completato negli ultimi {days} giorni."
        ));
    }
    let body = done
        .iter()
        .map(|t| {
            format!(
                "#{id} · {when} — {title}",
                id = t.id,
                when = format_schedule_human(&t.updated_at),
                title = t.title
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "Completati negli ultimi {days} giorni ({n}):\n{body}",
        n = done.len()
    ))
}

pub fn normalize_recurrence(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_lowercase().as_str() {
        "" | "none" | "no" | "una-tantum" | "once" => Ok("none"),
        "daily" | "giornaliero" | "giorno" | "ogni giorno" => Ok("daily"),
        "weekly" | "settimanale" | "settimana" | "ogni settimana" => Ok("weekly"),
        "monthly" | "mensile" | "mese" | "ogni mese" => Ok("monthly"),
        other => Err(format!(
            "ricorrenza non valida «{other}» (none|daily|weekly|monthly)"
        )),
    }
}

fn advance_schedule(schedule: &str, recurrence: &str) -> Result<String, String> {
    let base = parse_when(schedule).ok_or_else(|| "schedule non valido".to_string())?;
    let local = base.with_timezone(&Local);
    let next = match recurrence {
        "daily" => local + Duration::days(1),
        "weekly" => local + Duration::days(7),
        "monthly" => {
            let mut y = local.year();
            let mut m = local.month() + 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
            let day = local.day().min(28);
            local_at(
                NaiveDate::from_ymd_opt(y, m, day).ok_or("data mensile non valida")?,
                local.hour(),
                local.minute(),
            )?
        }
        _ => return Err("ricorrenza non anticipabile".into()),
    };
    // Keep advancing if still in the past (catch-up)
    let mut next = next;
    let now = Local::now();
    let mut guard = 0;
    while next <= now && guard < 400 {
        next = match recurrence {
            "daily" => next + Duration::days(1),
            "weekly" => next + Duration::days(7),
            "monthly" => {
                let mut y = next.year();
                let mut m = next.month() + 1;
                if m > 12 {
                    m = 1;
                    y += 1;
                }
                local_at(
                    NaiveDate::from_ymd_opt(y, m, next.day().min(28))
                        .ok_or("data mensile non valida")?,
                    next.hour(),
                    next.minute(),
                )?
            }
            _ => break,
        };
        guard += 1;
    }
    Ok(next.to_rfc3339())
}

pub fn list_reminders(db: &Db) -> Result<Vec<Reminder>, String> {
    let conn = db.lock()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, type, schedule, message, enabled, fired, created_at, task_id, recurrence
             FROM events
             ORDER BY fired ASC, schedule ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Reminder {
                id: row.get(0)?,
                r#type: row.get(1)?,
                schedule: row.get(2)?,
                message: row.get(3)?,
                enabled: row.get::<_, i64>(4)? == 1,
                fired: row.get::<_, i64>(5)? == 1,
                created_at: row.get(6)?,
                task_id: row.get(7)?,
                recurrence: row
                    .get::<_, Option<String>>(8)?
                    .unwrap_or_else(|| "none".into()),
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn create_reminder(
    db: &Db,
    schedule: &str,
    message: &str,
    task_id: Option<i64>,
    recurrence: Option<&str>,
) -> Result<i64, String> {
    let rfc = normalize_schedule(schedule)?;
    let message = message.trim();
    if message.is_empty() {
        return Err("messaggio promemoria vuoto".into());
    }
    let recurrence = normalize_recurrence(recurrence.unwrap_or("none"))?;
    if let Some(tid) = task_id {
        let ok = list_tasks(db)?.iter().any(|t| t.id == tid);
        if !ok {
            return Err(format!("task #{tid} non trovato"));
        }
    }
    let conn = db.lock()?;
    conn.execute(
        "INSERT INTO events (type, schedule, message, enabled, fired, created_at, task_id, recurrence)
         VALUES ('reminder', ?1, ?2, 1, 0, ?3, ?4, ?5)",
        params![rfc, message, now(), task_id, recurrence],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn create_reminder_parsed(db: &Db, parsed: &ParsedReminder) -> Result<i64, String> {
    create_reminder(
        db,
        &parsed.schedule_rfc3339,
        &parsed.message,
        parsed.task_id,
        Some(&parsed.recurrence),
    )
}

pub fn update_reminder(
    db: &Db,
    id: i64,
    schedule: Option<&str>,
    message: Option<&str>,
    task_id: Option<i64>,
    touch_task_id: bool,
    recurrence: Option<&str>,
) -> Result<(), String> {
    let mut rem = list_reminders(db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| format!("reminder #{id} non trovato"))?;
    if let Some(s) = schedule {
        rem.schedule = normalize_schedule(s)?;
    }
    if let Some(m) = message {
        let m = m.trim();
        if m.is_empty() {
            return Err("messaggio promemoria vuoto".into());
        }
        rem.message = m.to_string();
    }
    if touch_task_id {
        rem.task_id = task_id;
    }
    if let Some(r) = recurrence {
        rem.recurrence = normalize_recurrence(r)?.to_string();
    }
    let conn = db.lock()?;
    let n = conn
        .execute(
            "UPDATE events SET schedule = ?1, message = ?2, task_id = ?3, recurrence = ?4,
             enabled = 1, fired = 0 WHERE id = ?5",
            params![rem.schedule, rem.message, rem.task_id, rem.recurrence, id],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("reminder #{id} non trovato"));
    }
    Ok(())
}

pub fn delete_reminder(db: &Db, id: i64) -> Result<(), String> {
    let conn = db.lock()?;
    let n = conn
        .execute("DELETE FROM events WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("reminder #{id} non trovato"));
    }
    Ok(())
}

/// Postpone a reminder by `minutes` from now (re-enables if needed, clears fired).
pub fn snooze_reminder(db: &Db, id: i64, minutes: i64) -> Result<String, String> {
    if minutes <= 0 {
        return Err("i minuti di snooze devono essere positivi".into());
    }
    let when = Local::now() + Duration::minutes(minutes);
    let rfc = when.to_rfc3339();
    let conn = db.lock()?;
    let n = conn
        .execute(
            "UPDATE events SET schedule = ?1, enabled = 1, fired = 0 WHERE id = ?2 AND type = 'reminder'",
            params![rfc, id],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("reminder #{id} non trovato"));
    }
    Ok(rfc)
}

/// Snooze until tomorrow at `hour`:`minute` local (default 09:00).
pub fn snooze_reminder_until_tomorrow(
    db: &Db,
    id: i64,
    hour: u32,
    minute: u32,
) -> Result<String, String> {
    let date = Local::now().date_naive() + Duration::days(1);
    let when = local_at(date, hour, minute)?;
    reschedule_reminder(db, id, &when.to_rfc3339())
}

/// Reschedule a reminder to an absolute time (RFC3339 or YYYY-MM-DD HH:MM).
pub fn reschedule_reminder(db: &Db, id: i64, schedule: &str) -> Result<String, String> {
    let rfc = normalize_schedule(schedule)?;
    let conn = db.lock()?;
    let n = conn
        .execute(
            "UPDATE events SET schedule = ?1, enabled = 1, fired = 0 WHERE id = ?2 AND type = 'reminder'",
            params![rfc, id],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("reminder #{id} non trovato"));
    }
    Ok(rfc)
}

pub fn due_reminders(db: &Db) -> Result<Vec<Reminder>, String> {
    let all = list_reminders(db)?;
    let now_utc = Utc::now();
    Ok(all
        .into_iter()
        .filter(|r| r.enabled && !r.fired)
        .filter(|r| {
            parse_when(&r.schedule)
                .map(|dt| dt <= now_utc)
                .unwrap_or(false)
        })
        .collect())
}

/// Mark one-shot as fired, or advance recurring reminders to the next occurrence.
pub fn mark_fired_or_advance(db: &Db, id: i64) -> Result<(), String> {
    let rem = list_reminders(db)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| format!("reminder #{id} non trovato"))?;
    let recurrence = normalize_recurrence(&rem.recurrence)?;
    if recurrence == "none" {
        return mark_fired(db, id);
    }
    let next = advance_schedule(&rem.schedule, recurrence)?;
    let conn = db.lock()?;
    conn.execute(
        "UPDATE events SET schedule = ?1, fired = 0, enabled = 1 WHERE id = ?2",
        params![next, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn mark_fired(db: &Db, id: i64) -> Result<(), String> {
    let conn = db.lock()?;
    conn.execute("UPDATE events SET fired = 1 WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn tasks_for_label_query(db: &Db, query: &str) -> Result<String, String> {
    let q = query.trim().to_lowercase();
    let tasks = list_tasks(db)?;
    let open: Vec<_> = tasks
        .iter()
        .filter(|t| t.status == "open" || t.status == "doing")
        .filter(|t| {
            t.labels.iter().any(|l| {
                l.name.to_lowercase().contains(&q)
                    || format!("{}:{}", l.kind, l.name).to_lowercase().contains(&q)
            })
        })
        .collect();
    if open.is_empty() {
        return Ok(format!("Nessun task aperto con etichetta «{query}»."));
    }
    let body = open
        .iter()
        .map(|t| {
            let labs = t
                .labels
                .iter()
                .map(|l| format!("{}:{}", l.kind, l.name))
                .collect::<Vec<_>>()
                .join(", ");
            format!("#{} [{}] {}", t.id, labs, t.title)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("Task con «{query}» ({n}):\n{body}", n = open.len()))
}

pub fn format_schedule_human(schedule: &str) -> String {
    if let Some(dt) = parse_when(schedule) {
        let local = dt.with_timezone(&Local);
        return format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            local.year(),
            local.month(),
            local.day(),
            local.hour(),
            local.minute(),
            local.second()
        );
    }
    schedule.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tra_minuti() {
        let p = parse_reminder_nl("ricordami tra 5 minuti di controllare il server").unwrap();
        assert!(p.message.contains("controllare"));
        assert!(DateTime::parse_from_rfc3339(&p.schedule_rfc3339).is_ok());
    }

    #[test]
    fn parse_domani_alle() {
        let p = parse_reminder_nl("ricordami domani alle 9 di standup").unwrap();
        assert_eq!(p.message, "standup");
        assert!(p.human.contains("domani"));
    }

    #[test]
    fn parse_promemoria_explicit() {
        let p = parse_reminder_nl("promemoria: 2030-01-15 10:30 | backup").unwrap();
        assert_eq!(p.message, "backup");
        assert!(DateTime::parse_from_rfc3339(&p.schedule_rfc3339).is_ok());
    }

    #[test]
    fn parse_tra_secondi_conversational() {
        let p = parse_reminder_nl("puoi ricordarmi tra 90 secondi di bere acqua").unwrap();
        assert!(p.message.contains("bere"));
        let when = DateTime::parse_from_rfc3339(&p.schedule_rfc3339).unwrap();
        let delta = when.with_timezone(&Utc) - Utc::now();
        assert!(delta.num_seconds() >= 85 && delta.num_seconds() <= 95);

        let p2 = parse_reminder_nl("mi puoi ricordare tra 90 secondi di bere").unwrap();
        assert!(p2.message.contains("bere"));
    }

    #[test]
    fn parse_msg_then_tra() {
        let p = parse_reminder_nl("ricordami di chiamare il dentista tra 2 minuti").unwrap();
        assert!(p.message.contains("dentista"));
    }

    #[test]
    fn parse_tra_before_verb() {
        let p = parse_reminder_nl("tra 30 secondi ricordami di alzarmi").unwrap();
        assert!(p.message.contains("alzarmi"));
    }

    #[test]
    fn reminder_intent_heuristic() {
        assert!(looks_like_reminder_intent(
            "puoi ricordarmi tra 90 secondi di test"
        ));
        assert!(looks_like_reminder_intent("imposta un reminder tra 2 minuti"));
        assert!(!looks_like_reminder_intent("che tempo fa oggi"));
        assert!(!looks_like_reminder_intent("ricorda che odio il caffè"));
        assert!(looks_like_reminder_intent(
            "mi puoi ricordare tra 90 secondi di bere"
        ));
    }

    #[test]
    fn normalize_rejects_garbage() {
        assert!(normalize_schedule("not-a-date").is_err());
    }

    fn ready(title: &str, priority: i64) -> TaskCreate {
        TaskCreate::Ready {
            title: title.into(),
            priority,
            deadline: None,
            labels: vec![],
        }
    }

    #[test]
    fn parse_task_with_deadline_hint() {
        match parse_task_create("task: mandare report entro domani") {
            TaskCreate::Ready {
                title,
                priority,
                deadline: Some(dl),
                labels,
            } => {
                assert_eq!(title, "mandare report");
                assert_eq!(priority, 2);
                assert!(labels.is_empty());
                assert!(DateTime::parse_from_rfc3339(&dl).is_ok());
            }
            other => panic!("expected ready with deadline, got {other:?}"),
        }
    }

    #[test]
    fn parse_task_with_labels() {
        match parse_task_create("task: sistemare hero progetto sito #ui cliente Acme") {
            TaskCreate::Ready { title, labels, .. } => {
                assert_eq!(title, "sistemare hero");
                assert!(labels.iter().any(|l| l.kind == "project" && l.name == "sito"));
                assert!(labels.iter().any(|l| l.kind == "tag" && l.name == "ui"));
                assert!(labels.iter().any(|l| l.kind == "client" && l.name == "Acme"));
            }
            other => panic!("expected labeled task, got {other:?}"),
        }
        match parse_task_create("aggiungi al cliente Rossi: mandare preventivo") {
            TaskCreate::Ready { title, labels, .. } => {
                assert_eq!(title, "mandare preventivo");
                assert_eq!(labels.len(), 1);
                assert_eq!(labels[0].kind, "client");
                assert_eq!(labels[0].name, "Rossi");
            }
            other => panic!("expected client task, got {other:?}"),
        }
    }

    #[test]
    fn normalize_recurrence_ok() {
        assert_eq!(normalize_recurrence("settimanale").unwrap(), "weekly");
        assert_eq!(normalize_recurrence("none").unwrap(), "none");
    }

    #[test]
    fn advance_daily_moves_forward() {
        let past = (Local::now() - Duration::hours(2)).to_rfc3339();
        let next = advance_schedule(&past, "daily").unwrap();
        let dt = DateTime::parse_from_rfc3339(&next).unwrap();
        assert!(dt > Local::now());
    }

    #[test]
    fn extract_labels_hash_and_kinds() {
        let (title, specs) = extract_label_specs("fix hero progetto sito #ui cliente Acme");
        assert_eq!(title, "fix hero");
        assert!(specs.iter().any(|s| s.kind == "project" && s.name == "sito"));
        assert!(specs.iter().any(|s| s.kind == "tag" && s.name == "ui"));
        assert!(specs.iter().any(|s| s.kind == "client" && s.name == "Acme"));
    }

    #[test]
    fn parse_task_colon() {
        assert_eq!(
            parse_task_create("task: comprare latte"),
            ready("comprare latte", 2)
        );
        assert_eq!(
            parse_task_create("todo: backup server"),
            ready("backup server", 2)
        );
    }

    #[test]
    fn parse_task_salva_un_task() {
        assert_eq!(
            parse_task_create("salva un task: comprare il latte"),
            ready("comprare il latte", 2)
        );
        assert_eq!(
            parse_task_create("Puoi salvare un task per comprare il latte?"),
            ready("comprare il latte", 2)
        );
        assert_eq!(
            parse_task_create("crea un task chiamato fare la spesa"),
            ready("fare la spesa", 2)
        );
        assert_eq!(
            parse_task_create("aggiungi un task urgente: report"),
            ready("report", 1)
        );
        assert_eq!(parse_task_create("salva un task"), TaskCreate::NeedTitle);
        assert_eq!(
            parse_task_create("aggiungi un task comprare latte"),
            ready("comprare latte", 2)
        );
    }

    #[test]
    fn parse_task_ricorda_di() {
        assert_eq!(
            parse_task_create("ricorda di comprare il latte"),
            ready("comprare il latte", 2)
        );
        assert_eq!(
            parse_task_create("ricordami di chiamare il dentista"),
            ready("chiamare il dentista", 2)
        );
        assert_eq!(
            parse_task_create("remember to send the invoice"),
            ready("send the invoice", 2)
        );
        assert_eq!(
            parse_task_create("ricorda che odio il caffè"),
            TaskCreate::NotATask
        );
        assert_eq!(
            parse_task_create("ricordami tra 5 minuti di bere"),
            TaskCreate::NotATask
        );
        assert_eq!(parse_task_create("lista task"), TaskCreate::NotATask);
        assert!(!looks_like_task_create("lista task"));
        assert!(looks_like_task_create("salva un task: x"));
    }

    #[test]
    fn db_labels_and_recurring_reminder() {
        let db = crate::db::Db::open_in_memory().expect("mem db");
        let tid = create_task(
            &db,
            "invoice",
            "",
            1,
            None,
            &[],
            &[LabelSpec {
                kind: "client".into(),
                name: "Rossi".into(),
            }],
        )
        .unwrap();
        let tasks = list_tasks(&db).unwrap();
        let t = tasks.iter().find(|t| t.id == tid).unwrap();
        assert_eq!(t.labels.len(), 1);
        assert_eq!(t.labels[0].kind, "client");
        assert_eq!(t.labels[0].name, "Rossi");

        let past = (Local::now() - Duration::minutes(1))
            .format("%Y-%m-%d %H:%M")
            .to_string();
        let rid = create_reminder(&db, &past, "check", Some(tid), Some("daily")).unwrap();
        let due = due_reminders(&db).unwrap();
        assert!(due.iter().any(|r| r.id == rid));
        mark_fired_or_advance(&db, rid).unwrap();
        let after = list_reminders(&db).unwrap();
        let r = after.iter().find(|r| r.id == rid).unwrap();
        assert!(!r.fired);
        assert_eq!(r.task_id, Some(tid));
        assert_eq!(r.recurrence, "daily");
        let next = parse_when(&r.schedule).unwrap();
        assert!(next > Utc::now());
    }
}
