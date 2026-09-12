//! Morning briefing: tasks, reminders, memory highlights (+ optional LLM polish).

use crate::config::AppConfig;
use crate::db::Db;
use crate::llm::{self, ChatMessage};
use crate::memory;
use crate::organizer;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Timelike, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MorningBriefing {
    pub title: String,
    pub date_label: String,
    pub greeting: String,
    pub open_tasks: Vec<String>,
    pub upcoming_reminders: Vec<String>,
    pub memory_highlights: Vec<String>,
    pub narrative: String,
    pub speakable: String,
}

pub fn should_offer_now(cfg: &AppConfig) -> bool {
    if !cfg.briefing.enabled {
        return false;
    }
    let now = Local::now();
    let hour = now.hour();
    let start = cfg.briefing.hour.min(23);
    // Soft window: briefing hour ± 3h morning feel, or always if hour==0 (any time)
    if start == 0 {
        return true;
    }
    hour >= start && hour < start.saturating_add(4).min(24)
}

pub async fn build_briefing(cfg: &AppConfig, db: &Db) -> Result<MorningBriefing, String> {
    let now = Local::now();
    let date_label = now.format("%A %d %B %Y").to_string();
    let hour = now.hour();
    let greeting = if hour < 12 {
        "Buongiorno"
    } else if hour < 18 {
        "Buon pomeriggio"
    } else {
        "Buonasera"
    };

    let tasks = organizer::list_tasks(db)?;
    let open_tasks: Vec<String> = tasks
        .into_iter()
        .filter(|t| t.status != "done" && t.status != "cancelled")
        .take(8)
        .map(|t| {
            if t.priority > 0 {
                format!("[P{}] {}", t.priority, t.title)
            } else {
                t.title
            }
        })
        .collect();

    let reminders = organizer::list_reminders(db)?;
    let upcoming_reminders: Vec<String> = reminders
        .into_iter()
        .filter(|r| r.enabled && !r.fired)
        .filter_map(|r| {
            let when = parse_schedule(&r.schedule)?;
            Some((when, r.message))
        })
        .filter(|(when, _)| *when >= now.with_timezone(&Utc))
        .take(6)
        .map(|(when, msg)| {
            let local = when.with_timezone(&Local);
            format!("{} — {}", local.format("%d/%m %H:%M"), msg)
        })
        .collect();

    let snap = memory::snapshot(db, &cfg.memory)?;
    let mut memory_highlights = Vec::new();
    for e in snap.user.iter().chain(snap.memory.iter()).take(5) {
        let line: String = e.content.chars().take(120).collect();
        memory_highlights.push(line);
    }

    let mut narrative = String::new();
    narrative.push_str(&format!("{greeting}. Oggi è {date_label}.\n"));
    if open_tasks.is_empty() {
        narrative.push_str("Nessun task aperto in agenda.\n");
    } else {
        narrative.push_str(&format!(
            "Hai {} task aperti: {}.\n",
            open_tasks.len(),
            open_tasks.iter().take(3).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    if upcoming_reminders.is_empty() {
        narrative.push_str("Nessun promemoria in arrivo.\n");
    } else {
        narrative.push_str(&format!(
            "Prossimi promemoria: {}.\n",
            upcoming_reminders
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    // Optional LLM polish when briefing.llm_polish is on
    if cfg.briefing.llm_polish {
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "Riscrivi un briefing mattutino breve (max 80 parole) in italiano, \
                          caldo e pratico. Niente markdown. Usa solo i fatti forniti."
                    .into(),
            },
            ChatMessage {
                role: "user".into(),
                content: format!(
                    "Bozza:\n{narrative}\n\nTask:\n{}\n\nReminder:\n{}\n\nMemoria:\n{}",
                    open_tasks.join("\n"),
                    upcoming_reminders.join("\n"),
                    memory_highlights.join("\n")
                ),
            },
        ];
        if let Ok(polished) = llm::complete(cfg, &messages).await {
            let p = polished.trim().to_string();
            if !p.is_empty() {
                narrative = p;
            }
        }
    }

    let speakable = narrative.clone();
    Ok(MorningBriefing {
        title: "Briefing".into(),
        date_label,
        greeting: greeting.into(),
        open_tasks,
        upcoming_reminders,
        memory_highlights,
        narrative,
        speakable,
    })
}

fn parse_schedule(schedule: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(schedule) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(schedule, "%Y-%m-%d %H:%M") {
        return Local
            .from_local_datetime(&ndt)
            .single()
            .map(|d| d.with_timezone(&Utc));
    }
    None
}
