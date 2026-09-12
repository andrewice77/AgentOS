//! Procedural skills (agentskills.io-inspired): Markdown skills under ~/.agentos/skills/

use crate::config::data_dir;
use crate::llm::{self, ChatMessage};
use crate::config::AppConfig;
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    pub path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillReviewProposal {
    pub summary: String,
    pub suggested_skill_id: String,
    pub draft_markdown: String,
}

pub fn skills_dir() -> PathBuf {
    data_dir().join("skills")
}

pub fn ensure_skills_dir() -> Result<PathBuf, String> {
    let dir = skills_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Seed a starter skill once
    let starter = dir.join("companion-tone");
    if !starter.exists() {
        fs::create_dir_all(&starter).map_err(|e| e.to_string())?;
        fs::write(
            starter.join("SKILL.md"),
            r#"---
name: companion-tone
description: Tono e stile di risposta del compagno AgentOS
enabled: true
---

# Companion tone

- Rispondi in italiano salvo richiesta diversa.
- Sii conciso, pratico, cordiale — non servile.
- Se non sei sicuro, dillo chiaramente.
- Preferisci passi actionable a spiegazioni lunghe.
"#,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

fn parse_frontmatter(raw: &str) -> (serde_yaml::Value, String) {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return (serde_yaml::Value::Null, raw.to_string());
    }
    let rest = &trimmed[3..];
    if let Some(end) = rest.find("\n---") {
        let yaml = rest[..end].trim();
        let body = rest[end + 4..].trim_start().to_string();
        let meta = serde_yaml::from_str(yaml).unwrap_or(serde_yaml::Value::Null);
        (meta, body)
    } else {
        (serde_yaml::Value::Null, raw.to_string())
    }
}

fn meta_str(meta: &serde_yaml::Value, key: &str) -> Option<String> {
    meta.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn meta_bool(meta: &serde_yaml::Value, key: &str, default: bool) -> bool {
    meta.get(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

pub fn list_skills() -> Result<Vec<SkillMeta>, String> {
    let dir = ensure_skills_dir()?;
    let mut out = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }
        let id = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("skill")
            .to_string();
        let raw = fs::read_to_string(&skill_md).map_err(|e| e.to_string())?;
        let (meta, _) = parse_frontmatter(&raw);
        let name = meta_str(&meta, "name").unwrap_or_else(|| id.clone());
        let description = meta_str(&meta, "description").unwrap_or_default();
        let enabled = meta_bool(&meta, "enabled", true);
        out.push(SkillMeta {
            id,
            name,
            description,
            path: skill_md.display().to_string(),
            enabled,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub fn read_skill(id: &str) -> Result<String, String> {
    let path = skills_dir().join(id).join("SKILL.md");
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

pub fn write_skill(id: &str, markdown: &str) -> Result<(), String> {
    let id = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    if id.is_empty() {
        return Err("id skill vuoto".into());
    }
    let dir = skills_dir().join(&id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("SKILL.md"), markdown).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_skill(id: &str) -> Result<(), String> {
    let dir = skills_dir().join(id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Compact block for the system prompt (enabled skills only).
pub fn prompt_block() -> String {
    let Ok(skills) = list_skills() else {
        return String::new();
    };
    let enabled: Vec<_> = skills.into_iter().filter(|s| s.enabled).collect();
    if enabled.is_empty() {
        return String::new();
    }
    let mut block = String::from("\n\n## Active procedural skills\n");
    for s in enabled {
        if let Ok(raw) = read_skill(&s.id) {
            let (meta, body) = parse_frontmatter(&raw);
            let name = meta_str(&meta, "name").unwrap_or(s.name);
            let desc = meta_str(&meta, "description").unwrap_or(s.description);
            block.push_str(&format!("### {name}\n{desc}\n"));
            // Cap body to keep context small
            let excerpt: String = body.chars().take(800).collect();
            block.push_str(&excerpt);
            block.push_str("\n\n");
        }
    }
    block
}

pub async fn run_background_review(
    cfg: &AppConfig,
    db: &Db,
) -> Result<SkillReviewProposal, String> {
    ensure_skills_dir()?;
    let skills = list_skills()?;
    let skill_summaries = skills
        .iter()
        .map(|s| format!("- {} ({}): {}", s.id, s.name, s.description))
        .collect::<Vec<_>>()
        .join("\n");

    let recent = {
        let conn = db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT role, content FROM messages ORDER BY id DESC LIMIT 30",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        rows
    };

    let mut transcript = String::new();
    for (role, content) in recent.into_iter().rev() {
        let snippet: String = content.chars().take(280).collect();
        transcript.push_str(&format!("{role}: {snippet}\n"));
    }
    if transcript.trim().is_empty() {
        transcript = "(nessun messaggio recente)".into();
    }

    let prompt = format!(
        "Sei il modulo di self-improvement di AgentOS.\n\
         Analizza le chat recenti e le skill esistenti. Proponi UNA skill procedurale nuova o aggiornata \
         in formato agentskills (Markdown con frontmatter YAML: name, description, enabled).\n\
         Rispondi SOLO in JSON con chiavi: summary, suggested_skill_id, draft_markdown.\n\
         draft_markdown deve essere il file SKILL.md completo.\n\n\
         Skill esistenti:\n{skill_summaries}\n\n\
         Chat recenti:\n{transcript}"
    );

    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: "Output only valid JSON. No markdown fences.".into(),
        },
        ChatMessage {
            role: "user".into(),
            content: prompt,
        },
    ];
    let raw = llm::complete(cfg, &messages).await?;
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let parsed: SkillReviewProposal =
        serde_json::from_str(cleaned).map_err(|e| format!("review parse: {e}; raw={cleaned}"))?;
    // Persist proposal draft for UI
    let review_dir = skills_dir().join("_review");
    fs::create_dir_all(&review_dir).map_err(|e| e.to_string())?;
    fs::write(
        review_dir.join("latest.json"),
        serde_json::to_string_pretty(&parsed).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(parsed)
}

pub fn latest_review() -> Result<Option<SkillReviewProposal>, String> {
    let path = skills_dir().join("_review").join("latest.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let p: SkillReviewProposal = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(Some(p))
}
