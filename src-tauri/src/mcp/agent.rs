//! Generic MCP tool-calling loop: the model picks any discovered tool.
//! No domain knowledge (mail, finance, …) — only the live tool catalog.

use super::McpToolInfo;
use crate::agent::AppState;
use crate::config::AppConfig;
use crate::llm::{self, ChatMessage};
use regex::Regex;
use serde_json::{json, Value};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};

const MAX_ROUNDS: usize = 8;
const MAX_RESULT_CHARS: usize = 3500;
const MAX_TOOLS_IN_PROMPT: usize = 48;
const MAX_PLAN_NUDGES: usize = 3;

#[derive(Debug, Clone)]
pub struct ParsedCall {
    pub server: String,
    pub tool: String,
    pub arguments: Value,
}

#[derive(Debug)]
pub enum LoopOutcome {
    /// Final natural-language answer (no more tool calls).
    Answer(String),
    /// Permission gate blocked a call — user must `approva mcp`.
    NeedsApproval { server: String, tool: String, level: String },
}

/// Instructions + catalog injected into the system prompt when MCP tools exist.
pub fn tools_system_block(tools: &[McpToolInfo]) -> String {
    if tools.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n\n## External MCP tools\n\
         You have live tools from connected MCP servers.\n\
         Rules (strict):\n\
         - When you need data or an action from a tool, respond with ONLY an mcp-call fence — \
         no plan, no preamble, no bullet list of steps.\n\
         ```mcp-call\n\
         {\"server\":\"SERVER\",\"tool\":\"TOOL_NAME\",\"arguments\":{...}}\n\
         ```\n\
         - Multi-step workflows (e.g. portal → project → tasks): call one tool per turn; \
         after each TOOL RESULT, either emit the next mcp-call or give the final answer.\n\
         - Never invent IDs, task lists, or statuses. Use tool results only.\n\
         - Do NOT ask the user for numeric IDs if you can resolve a name from prior TOOL RESULT \
         context (projects, tasks, issues, accounts, …) or with a short lookup tool call.\n\
         - After enough TOOL RESULT evidence, answer the user clearly (Italian if they write Italian). \
         Use short markdown headings (## / ###) for sections the user asked about, then bullet lists \
         for the key points. Summarize: do not dump raw JSON.\n\
         - If no tool is needed, answer normally without an mcp-call block.\n\
         - Never invent credentials or bypass user approval.\n\n\
         Available tools (use exact server/tool names):\n",
    );
    for t in tools.iter().take(MAX_TOOLS_IN_PROMPT) {
        let desc: String = t.description.chars().take(160).collect();
        out.push_str(&format!("- {}/{} — {desc}\n", t.server, t.name));
        if let Some(hint) = schema_arg_hint(&t.input_schema) {
            out.push_str(&format!("  args: {hint}\n"));
        }
    }
    out
}

fn schema_arg_hint(schema: &Value) -> Option<String> {
    if schema.is_null() {
        return None;
    }
    let props = schema
        .pointer("/properties")
        .or_else(|| schema.get("properties"))?;
    let mut names: Vec<&str> = props
        .as_object()?
        .keys()
        .map(|k| k.as_str())
        .take(8)
        .collect();
    if names.is_empty() {
        // Zoho-style nested path_variables
        if let Some(pv) = props.get("path_variables").and_then(|p| p.get("properties")) {
            names = pv
                .as_object()?
                .keys()
                .map(|k| k.as_str())
                .take(8)
                .collect();
            if !names.is_empty() {
                return Some(format!("path_variables.{{{}}}", names.join(",")));
            }
        }
        let raw = serde_json::to_string(schema).ok()?;
        let cut: String = raw.chars().take(220).collect();
        return Some(cut);
    }
    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .take(6)
                .collect()
        })
        .unwrap_or_default();
    if required.is_empty() {
        Some(format!("{{{}}}", names.join(", ")))
    } else {
        Some(format!(
            "required [{}]; also {{{}}}",
            required.join(", "),
            names.join(", ")
        ))
    }
}

/// Extract a single tool call from model output, if present.
pub fn parse_tool_call(text: &str) -> Option<ParsedCall> {
    let trimmed = text.trim();
    if let Some(c) = parse_fenced_mcp(trimmed) {
        return Some(c);
    }
    if let Some(c) = parse_xml_mcp(trimmed) {
        return Some(c);
    }
    // Whole-message JSON
    if trimmed.starts_with('{') {
        if let Some(c) = parse_call_json(trimmed) {
            return Some(c);
        }
    }
    // Prose + fence somewhere in the message (small models often add preamble)
    if trimmed.len() > 20 {
        if let Some(c) = parse_fenced_mcp(trimmed) {
            return Some(c);
        }
    }
    None
}

fn parse_fenced_mcp(text: &str) -> Option<ParsedCall> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?is)```\s*mcp(?:-call)?\s*\n(.*?)```").expect("mcp fence regex")
    });
    let caps = re.captures(text)?;
    parse_call_json(caps.get(1)?.as_str().trim())
}

fn parse_xml_mcp(text: &str) -> Option<ParsedCall> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r#"(?is)<mcp_call\s+server=["']([^"']+)["']\s+tool=["']([^"']+)["']\s*>(.*?)</mcp_call>"#,
        )
        .expect("mcp xml regex")
    });
    let caps = re.captures(text)?;
    let server = caps.get(1)?.as_str().trim().to_string();
    let tool = caps.get(2)?.as_str().trim().to_string();
    let body = caps.get(3)?.as_str().trim();
    let arguments = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_str(body).unwrap_or_else(|_| json!({ "text": body }))
    };
    Some(ParsedCall {
        server,
        tool,
        arguments,
    })
}

fn parse_call_json(raw: &str) -> Option<ParsedCall> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let server = v
        .get("server")
        .or_else(|| v.get("mcp_server"))
        .and_then(|x| x.as_str())?
        .trim()
        .to_string();
    let tool = v
        .get("tool")
        .or_else(|| v.get("name"))
        .and_then(|x| x.as_str())?
        .trim()
        .to_string();
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    // Allow "server/tool" packed into tool or name field
    let (server, tool) = if tool.contains('/') && v.get("server").and_then(|x| x.as_str()).is_none()
    {
        match tool.split_once('/') {
            Some((s, t)) => (s.to_string(), t.to_string()),
            None => (server, tool),
        }
    } else {
        (server, tool)
    };
    let arguments = v
        .get("arguments")
        .or_else(|| v.get("args"))
        .or_else(|| v.get("parameters"))
        .cloned()
        .unwrap_or(json!({}));
    Some(ParsedCall {
        server,
        tool,
        arguments,
    })
}

/// Compact MCP tool payloads for the model (prefer structured/text, cap size).
pub fn truncate_result(v: &Value) -> String {
    let compact = compact_mcp_payload(v);
    if compact.chars().count() <= MAX_RESULT_CHARS {
        compact
    } else {
        let cut: String = compact.chars().take(MAX_RESULT_CHARS).collect();
        format!("{cut}\n…[truncated]")
    }
}

fn compact_mcp_payload(v: &Value) -> String {
    if let Some(sc) = v.get("structuredContent").or_else(|| v.get("structured_content")) {
        if let Ok(s) = serde_json::to_string_pretty(sc) {
            return s;
        }
    }
    if let Some(arr) = v.get("content").and_then(|c| c.as_array()) {
        let mut parts = Vec::new();
        for item in arr {
            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                let trimmed = t.trim();
                if let Ok(inner) = serde_json::from_str::<Value>(trimmed) {
                    parts.push(serde_json::to_string_pretty(&inner).unwrap_or_else(|_| trimmed.into()));
                } else {
                    parts.push(trimmed.to_string());
                }
            }
        }
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

/// True when the model is narrating a plan instead of calling a tool / finishing.
pub fn looks_like_plan_without_tool(text: &str) -> bool {
    let t = text.to_lowercase();
    let markers = [
        "procedo",
        "prima chiamata",
        "prima ottengo",
        "devo prima",
        "sequenza",
        "passo 1",
        "step 1",
        "otterò",
        "otterrò",
        "recupererò",
        "chiamata per",
        "portal_id",
        "project_id",
        "mcp-call",
        "eseguirò",
        "eseguire",
        "lista dei portali",
        "identificativo numerico",
    ];
    let hits = markers.iter().filter(|m| t.contains(*m)).count();
    hits >= 2 || (hits >= 1 && text.chars().count() > 280)
}

fn looks_incomplete_answer(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    let chars = t.chars().count();
    if chars < 80 {
        return true;
    }
    // Cut mid-sentence / mid-list / mid-JSON
    let last = t.chars().last().unwrap_or('.');
    if matches!(last, ',' | ':' | '—' | '-' | '(' | '"' | '{' | '[' | '\\' | '`') {
        return true;
    }
    if looks_like_broken_mcp_call(t) {
        return true;
    }
    let lower = t.to_lowercase();
    lower.ends_with(" ma")
        || lower.ends_with(" ma la")
        || lower.contains("ho recuperato") && chars < 200
        || lower.contains("nome:") && !lower.contains("task")
}

/// Model started an mcp-call / JSON tool payload but never finished it.
pub fn looks_like_broken_mcp_call(text: &str) -> bool {
    if parse_tool_call(text).is_some() {
        return false;
    }
    let t = text.trim();
    let lower = t.to_lowercase();
    let starts_fence = lower.contains("```mcp") || lower.contains("``` mcp");
    let starts_json = t.contains("\"server\"") && t.contains("zoho")
        || t.contains("\"server\"") && t.contains("\"tool\"");
    let unclosed_fence = t.matches("```").count() == 1;
    let unclosed_json = t.matches('{').count() > t.matches('}').count();
    (starts_fence || starts_json) && (unclosed_fence || unclosed_json || !t.contains("```"))
}


/// Resolve portal/project from the latest message, history blob, and session ctx.
fn resolve_zoho_ids(
    latest: &str,
    history: &str,
    ctx: Option<&crate::mcp::zoho_ctx::ZohoCtx>,
) -> (Option<String>, Option<String>) {
    static RE_PORTAL: OnceLock<Regex> = OnceLock::new();
    static RE_PROJECT: OnceLock<Regex> = OnceLock::new();
    let re_portal = RE_PORTAL.get_or_init(|| {
        Regex::new(r#"(?i)portal[_\s-]*id\s*[=:]\s*["']?(\d{6,})"#).expect("portal re")
    });
    let re_project = RE_PROJECT.get_or_init(|| {
        Regex::new(r#"(?i)project[_\s-]*id\s*[=:]\s*["']?(\d{6,})"#).expect("project re")
    });

    let mut portal = re_portal
        .captures(latest)
        .or_else(|| re_portal.captures(history))
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
    let mut project = re_project
        .captures(latest)
        .or_else(|| re_project.captures(history))
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));

    if let Some(c) = ctx {
        if portal.is_none() {
            portal = c.portal_id.clone();
        }
        if project.is_none() {
            project = c.project_id.clone();
        }
        // Soft name→id: "task del progetto Alpha" using cached entities
        if project.is_none() {
            project = c.resolve_id_by_kind("project", latest);
        }
        if portal.is_none() {
            portal = c.resolve_id_by_kind("portal", latest);
        }
        if project.is_none() {
            if let Some(e) = c.resolve_any_named(latest) {
                if e.kind == "project" || e.kind == "entity" {
                    project = Some(e.id.clone());
                }
            }
        }
    }

    // Bare IDs in "Portal_id X, project_id Y" style already handled; also pick labeled pairs from history.
    let digit_re = Regex::new(r"\b(\d{10,})\b").ok();
    if let Some(re) = digit_re {
        let mut ids: Vec<String> = re
            .captures_iter(latest)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect();
        ids.dedup();
        let lower = latest.to_lowercase();
        if portal.is_none() && lower.contains("portal") && !ids.is_empty() {
            portal = Some(ids[0].clone());
        }
        if project.is_none() {
            if ids.len() >= 2 {
                project = Some(ids.last().cloned().unwrap_or_default());
            } else if lower.contains("project") && ids.len() == 1 {
                project = Some(ids[0].clone());
            }
        }
    }
    (portal, project)
}

fn looks_like_issue_detail(latest: &str) -> Option<String> {
    let lower = latest.to_lowercase();
    let detail_words = [
        "dettagli",
        "dettasgli",
        "dettaglio",
        "dettaglo",
        "descrizione",
        "apri",
        "mostra",
        "info",
        "informazioni",
        "questo",
        "questa",
        "quello",
        "quella",
        "details",
        "detail",
        "about",
    ];
    let wants_detail = detail_words.iter().any(|w| lower.contains(w));
    let re = Regex::new(r"\b(\d{10,})\b").ok()?;
    let ids: Vec<String> = re
        .captures_iter(latest)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect();
    // Single long ID + detail wording, or "ID: 123…" / bare id as whole focus
    if ids.len() == 1 && (wants_detail || latest.trim().chars().filter(|c| c.is_ascii_digit()).count() >= 10) {
        return Some(ids[0].clone());
    }
    if wants_detail && ids.len() >= 1 {
        // Prefer last id in the message (usually the one they pasted)
        return ids.last().cloned();
    }
    None
}

fn soft_issue_id_from_ctx(
    latest: &str,
    ctx: Option<&crate::mcp::zoho_ctx::ZohoCtx>,
) -> Option<String> {
    let c = ctx?;
    let lower = latest.to_lowercase();
    let detail_words = ["dettagli", "dettaglio", "apri", "mostra", "info", "details", "about"];
    if !detail_words.iter().any(|w| lower.contains(w)) {
        return None;
    }
    c.resolve_id_by_kind("issue", latest)
        .or_else(|| c.resolve_id_by_kind("task", latest))
}

/// If the user already provided numeric portal/project IDs and asks for issues/tasks,
/// build the MCP call without relying on the local model (avoids truncated JSON).
pub fn try_deterministic_zoho_call(
    tools: &[McpToolInfo],
    latest_user: &str,
    history_blob: &str,
    ctx: Option<&crate::mcp::zoho_ctx::ZohoCtx>,
) -> Option<ParsedCall> {
    let lower = latest_user.to_lowercase();
    let (portal_id, project_id) = resolve_zoho_ids(latest_user, history_blob, ctx);

    // Follow-up: "dettagli di questo: <issue_id>" or soft name match
    if let Some(issue_id) = looks_like_issue_detail(latest_user)
        .or_else(|| soft_issue_id_from_ctx(latest_user, ctx))
    {
        let portal_id = portal_id?;
        let project_id = project_id?;
        // Don't treat portal/project ids themselves as issue ids
        if issue_id == portal_id || issue_id == project_id {
            return None;
        }
        let tool = tools.iter().find(|t| t.name == "ZohoProjects_get_issue")?;
        return Some(ParsedCall {
            server: tool.server.clone(),
            tool: tool.name.clone(),
            arguments: json!({
                "path_variables": {
                    "portal_id": portal_id,
                    "project_id": project_id,
                    "issue_id": issue_id,
                }
            }),
        });
    }

    let wants_issues = ["issue", "bug", "ticket"].iter().any(|k| lower.contains(k));
    let wants_tasks = lower.contains("task");
    // Also allow list intent from history only when latest is empty of intent? No — latest must ask.
    if !wants_issues && !wants_tasks {
        return None;
    }

    let portal_id = portal_id?;
    let project_id = project_id?;

    if wants_issues {
        let tool = tools.iter().find(|t| t.name == "ZohoProjects_get_all_issues")?;
        let filter = serde_json::json!({
            "criteria": [{
                "field_name": "project",
                "criteria_condition": "is",
                "value": [project_id]
            }],
            "pattern": "1"
        });
        return Some(ParsedCall {
            server: tool.server.clone(),
            tool: tool.name.clone(),
            arguments: json!({
                "path_variables": { "portal_id": portal_id },
                "query_params": {
                    "page": "1",
                    "per_page": "100",
                    "filter": filter.to_string()
                }
            }),
        });
    }

    let tool = tools
        .iter()
        .find(|t| t.name == "ZohoProjects_get_tasks_by_project")?;
    let filter = serde_json::json!({
        "criteria": [{
            "field_name": "is_completed",
            "criteria_condition": "is",
            "value": ["0"]
        }],
        "pattern": "1"
    });
    Some(ParsedCall {
        server: tool.server.clone(),
        tool: tool.name.clone(),
        arguments: json!({
            "path_variables": {
                "portal_id": portal_id,
                "project_id": project_id,
            },
            "query_params": {
                "page": "1",
                "per_page": "100",
                "filter": filter.to_string()
            }
        }),
    })
}

/// Turn a successful Zoho list/detail payload into an Italian summary (no LLM).
pub fn format_zoho_list_answer(tool: &str, result: &Value) -> Option<String> {
    let sc = result
        .get("structuredContent")
        .or_else(|| result.get("structured_content"))
        .cloned()
        .or_else(|| Some(result.clone()))?;
    if sc.get("status").and_then(|s| s.as_str()) == Some("failure") {
        let msg = sc
            .pointer("/data/message")
            .or_else(|| sc.pointer("/data/error/title"))
            .and_then(|m| m.as_str())
            .unwrap_or("errore API");
        return Some(format!("Zoho ha restituito un errore: {msg}"));
    }
    let data = sc.get("data")?;

    // Single issue: get_issue → data.result[0]
    if tool.contains("get_issue") && !tool.contains("all_issues") {
        let issue = data
            .get("result")
            .and_then(|r| r.as_array())
            .and_then(|a| a.first())
            .or_else(|| data.get("issue"))?;
        return Some(format_one_issue(issue));
    }

    if tool.contains("issue") {
        let issues = data.get("issues")?.as_array()?;
        let open: Vec<&Value> = issues
            .iter()
            .filter(|i| {
                i.pointer("/status/is_closed_type")
                    .and_then(|v| v.as_bool())
                    != Some(true)
            })
            .collect();
        if open.is_empty() {
            return Some(format!(
                "Nessun issue aperto tra i {} restituiti per questo filtro progetto.",
                issues.len()
            ));
        }
        let mut lines = vec![format!(
            "Issues aperti: {} (su {} in pagina).\nContesto salvato: puoi chiedere i dettagli di un ID senza ripetere portal/project.",
            open.len(),
            issues.len()
        )];
        for (idx, i) in open.iter().take(40).enumerate() {
            let name = i
                .get("name")
                .or_else(|| i.get("title"))
                .and_then(|n| n.as_str())
                .unwrap_or("(senza titolo)");
            let name = html_unescape_basic(name);
            let id = i.get("id").and_then(|n| n.as_str()).unwrap_or("?");
            let st = i
                .pointer("/status/name")
                .and_then(|n| n.as_str())
                .unwrap_or("?");
            lines.push(format!("{}. [{}] {} — {}", idx + 1, id, name, st.trim()));
        }
        if open.len() > 40 {
            lines.push(format!("… e altri {}.", open.len() - 40));
        }
        return Some(lines.join("\n"));
    }

    if tool.contains("task") {
        let tasks = data.get("tasks")?.as_array()?;
        let open: Vec<&Value> = tasks
            .iter()
            .filter(|t| {
                t.get("is_completed").and_then(|v| v.as_bool()) != Some(true)
                    && t.pointer("/status/is_closed_type").and_then(|v| v.as_bool())
                        != Some(true)
            })
            .collect();
        let list: Vec<&Value> = if open.is_empty() {
            tasks.iter().collect()
        } else {
            open
        };
        let mut lines = vec![format!(
            "Task aperti: {}.\nContesto salvato: puoi chiedere dettagli di un ID senza ripetere portal/project.",
            list.len()
        )];
        for (idx, t) in list.iter().take(40).enumerate() {
            let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("(senza titolo)");
            let name = html_unescape_basic(name);
            let prefix = t.get("prefix").and_then(|n| n.as_str()).unwrap_or("");
            let st = t
                .pointer("/status/name")
                .and_then(|n| n.as_str())
                .unwrap_or("?");
            let id = t.get("id").and_then(|n| n.as_str()).unwrap_or("?");
            lines.push(format!(
                "{}. {} {} — {} [{}]",
                idx + 1,
                prefix,
                name,
                st.trim(),
                id
            ));
        }
        if list.len() > 40 {
            lines.push(format!("… e altri {}.", list.len() - 40));
        }
        return Some(lines.join("\n"));
    }
    None
}

fn format_one_issue(issue: &Value) -> String {
    let name = issue
        .get("name")
        .or_else(|| issue.get("title"))
        .and_then(|n| n.as_str())
        .unwrap_or("(senza titolo)");
    let name = html_unescape_basic(name);
    let id = issue.get("id").and_then(|n| n.as_str()).unwrap_or("?");
    let prefix = issue.get("prefix").and_then(|n| n.as_str()).unwrap_or("");
    let st = issue
        .pointer("/status/name")
        .and_then(|n| n.as_str())
        .unwrap_or("?");
    let project = issue
        .pointer("/project/name")
        .and_then(|n| n.as_str())
        .unwrap_or("?");
    let assignee = issue
        .pointer("/assignee/name")
        .and_then(|n| n.as_str())
        .unwrap_or("?");
    let created = issue
        .get("created_time")
        .and_then(|n| n.as_str())
        .unwrap_or("?");
    let created_by = issue
        .pointer("/created_by/name")
        .and_then(|n| n.as_str())
        .unwrap_or("?");
    let priority = issue
        .pointer("/severity/value")
        .and_then(|n| n.as_str())
        .unwrap_or("?");
    let desc = issue
        .get("description")
        .and_then(|n| n.as_str())
        .map(html_unescape_basic)
        .filter(|s| !s.trim().is_empty());

    let mut lines = vec![
        format!("Issue {} {}", prefix, name).trim().to_string(),
        format!("ID: {id}"),
        format!("Progetto: {project}"),
        format!("Stato: {}", st.trim()),
        format!("Priorità: {priority}"),
        format!("Assegnato a: {assignee}"),
        format!("Creato da: {created_by} il {created}"),
    ];
    if let Some(d) = desc {
        let cut: String = d.chars().take(1200).collect();
        lines.push(format!("Descrizione:\n{cut}"));
    }
    lines.join("\n")
}

fn html_unescape_basic(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("<br />", " ")
        .replace("<br/>", " ")
}

fn summarize_last_tool_result(messages: &[ChatMessage], tool_name: &str) -> Option<String> {
    let last = messages.iter().rev().find(|msg| {
        msg.role == "user" && msg.content.starts_with("TOOL RESULT from ")
    })?;
    let raw = last
        .content
        .split_once(":\n")
        .map(|(_, rest)| rest.split("\n\nIf you still").next().unwrap_or(rest))
        .unwrap_or("")
        .trim();
    let v: Value = serde_json::from_str(raw).ok()?;
    format_zoho_list_answer(tool_name, &v)
}

fn remember_zoho_from_call(state: &AppState, session_id: &str, call: &ParsedCall) {
    let pv = call.arguments.get("path_variables");
    let portal = pv
        .and_then(|p| p.get("portal_id"))
        .and_then(|v| v.as_str());
    let mut project = pv
        .and_then(|p| p.get("project_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if project.is_none() {
        if let Some(filt) = call
            .arguments
            .pointer("/query_params/filter")
            .and_then(|f| f.as_str())
        {
            if let Ok(v) = serde_json::from_str::<Value>(filt) {
                project = v
                    .pointer("/criteria/0/value/0")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
            }
        }
    }
    let project_ref = project.as_deref();
    if portal.is_some() || project_ref.is_some() {
        state
            .zoho_ctx
            .upsert_ids(&state.db, session_id, portal, project_ref);
        crate::debuglog::info(
            None,
            format!(
                "mcp-agent:zoho_ctx portal={:?} project={:?}",
                portal, project_ref
            ),
        );
    }
}

/// Run up to MAX_ROUNDS of: LLM → optional mcp-call → execute → feed result.
///
/// `skip_approval`: after the user approved once (`approva mcp`), continue the chain
/// without re-prompting for each subsequent tool in this loop.
pub async fn run_loop(
    app: &AppHandle,
    state: &AppState,
    cfg: &AppConfig,
    mut messages: Vec<ChatMessage>,
    tools: &[McpToolInfo],
    session_id: &str,
    skip_approval: bool,
) -> Result<LoopOutcome, String> {
    if tools.is_empty() {
        let reply = llm::complete_tools(cfg, &messages).await?;
        return Ok(LoopOutcome::Answer(reply));
    }

    let mut plan_nudges = 0usize;
    let mut saw_tool_result = messages.iter().any(|m| {
        m.role == "user" && m.content.starts_with("TOOL RESULT from ")
    });

    let zoho_ctx = state.zoho_ctx.get_for_session(&state.db, session_id);
    let latest_user = messages
        .iter()
        .rev()
        .find(|m| m.role == "user" && !m.content.starts_with("TOOL RESULT") && !m.content.starts_with("TOOL ERROR"))
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let history_blob = messages
        .iter()
        .rev()
        .take(12)
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");

    // Inject remembered entities into the system message once.
    if let Some(hint) = zoho_ctx.as_ref().map(|c| c.prompt_hint()) {
        if !hint.is_empty() {
            if let Some(sys) = messages.iter_mut().find(|m| m.role == "system") {
                if !sys.content.contains("## Remembered entities") {
                    sys.content.push_str(&hint);
                }
            }
        }
    }

    // Fast path: IDs + intent — don't trust small local models to emit full JSON.
    // Skip after approval resume (TOOL RESULT already present).
    if !saw_tool_result {
        if let Some(call) = try_deterministic_zoho_call(
            tools,
            &latest_user,
            &history_blob,
            zoho_ctx.as_ref(),
        ) {
            crate::debuglog::info(
                Some(app),
                format!(
                    "mcp-agent:deterministic {}/{}",
                    call.server, call.tool
                ),
            );
            remember_zoho_from_call(state, session_id, &call);
            let tool_name = call.tool.clone();
            let reply = format!(
                "```mcp-call\n{}\n```",
                serde_json::json!({
                    "server": call.server,
                    "tool": call.tool,
                    "arguments": call.arguments,
                })
            );
            match run_one_call(
                app,
                state,
                cfg,
                messages.clone(),
                tools,
                skip_approval,
                reply,
                call,
                &mut saw_tool_result,
                0,
                session_id,
            )
            .await?
            {
                OneCall::Continue(m) => {
                    if let Some(answer) = summarize_last_tool_result(&m, &tool_name) {
                        let _ = app.emit("chat-status", "");
                        return Ok(LoopOutcome::Answer(answer));
                    }
                    messages = m;
                }
                OneCall::Done(out) => return Ok(out),
            }
        }
    }

    for round in 0..MAX_ROUNDS {
        crate::debuglog::info(
            Some(app),
            format!(
                "mcp-agent:round {} messages={} skip_approval={skip_approval}",
                round + 1,
                messages.len()
            ),
        );
        let _ = app.emit(
            "chat-status",
            if round == 0 {
                "Pensando (tool MCP)…"
            } else {
                "Integro i risultati…"
            },
        );

        let reply = llm::complete_tools(cfg, &messages).await?;
        let Some(call) = parse_tool_call(&reply) else {
            let broken = looks_like_broken_mcp_call(&reply);
            if plan_nudges < MAX_PLAN_NUDGES
                && (broken
                    || looks_like_plan_without_tool(&reply)
                    || looks_incomplete_answer(&reply)
                    || (!saw_tool_result && reply.chars().count() < 120))
            {
                plan_nudges += 1;
                crate::debuglog::info(
                    Some(app),
                    format!(
                        "mcp-agent:nudge count={plan_nudges} broken={broken} chars={}",
                        reply.chars().count()
                    ),
                );
                messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: if broken {
                        "[incomplete mcp-call omitted]".into()
                    } else {
                        reply
                    },
                });
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: if broken {
                        "Your previous mcp-call JSON was CUT OFF / invalid. Emit ONE complete                          ```mcp-call``` block only, with valid closed JSON (server, tool, arguments).                          No prose before or after."
                            .into()
                    } else if !saw_tool_result {
                        "Stop explaining. Emit ONLY one complete ```mcp-call``` block now                          (valid JSON, closed fences). No other text."
                            .into()
                    } else {
                        "Your answer looks incomplete. Either emit the next complete ```mcp-call```                          or write a full Italian summary from TOOL RESULT evidence. No raw JSON."
                            .into()
                    },
                });
                continue;
            }
            let _ = app.emit("chat-status", "");
            if broken {
                return Ok(LoopOutcome::Answer(
                    "Non sono riuscito a completare la chiamata allo strumento (risposta tagliata).                      Apri una chat nuova e ripeti la richiesta con portal_id e project_id."
                        .into(),
                ));
            }
            return Ok(LoopOutcome::Answer(strip_residual_fences(&reply)));
        };

        // Validate tool exists; allow unique tool-name match if server is wrong
        let call = {
            let exact = tools
                .iter()
                .any(|t| t.server == call.server && t.name == call.tool);
            if exact {
                call
            } else {
                let matches: Vec<_> = tools
                    .iter()
                    .filter(|t| {
                        t.name == call.tool
                            || t.name.ends_with(&format!("_{}", call.tool))
                            || t.name.rsplit('_').next() == Some(call.tool.as_str())
                    })
                    .collect();
                if matches.len() == 1 {
                    ParsedCall {
                        server: matches[0].server.clone(),
                        tool: matches[0].name.clone(),
                        arguments: call.arguments,
                    }
                } else {
                    messages.push(ChatMessage {
                        role: "assistant".into(),
                        content: reply,
                    });
                    messages.push(ChatMessage {
                        role: "user".into(),
                        content: format!(
                            "TOOL ERROR: unknown tool {}/{}. Pick one from the Available tools list \
                             (exact server and tool names).",
                            call.server, call.tool
                        ),
                    });
                    continue;
                }
            }
        };

        match run_one_call(
            app,
            state,
            cfg,
            messages,
            tools,
            skip_approval,
            reply,
            call,
            &mut saw_tool_result,
            round,
            session_id,
        )
        .await?
        {
            OneCall::Continue(m) => messages = m,
            OneCall::Done(out) => return Ok(out),
        }
    }

    let _ = app.emit("chat-status", "");
    messages.push(ChatMessage {
        role: "user".into(),
        content: "Stop calling tools. Give the best complete answer you can in Italian with the \
                  information so far. No raw JSON."
            .into(),
    });
    let reply = llm::complete_tools(cfg, &messages).await?;
    Ok(LoopOutcome::Answer(strip_residual_fences(&reply)))
}

enum OneCall {
    Continue(Vec<ChatMessage>),
    Done(LoopOutcome),
}

async fn run_one_call(
    app: &AppHandle,
    state: &AppState,
    cfg: &AppConfig,
    mut messages: Vec<ChatMessage>,
    _tools: &[McpToolInfo],
    skip_approval: bool,
    reply: String,
    call: ParsedCall,
    saw_tool_result: &mut bool,
    _round: usize,
    session_id: &str,
) -> Result<OneCall, String> {
    let _ = app.emit(
        "chat-status",
        format!("MCP {}/{}…", call.server, call.tool),
    );
    crate::debuglog::info(
        Some(app),
        format!("mcp-agent:call {}/{}", call.server, call.tool),
    );

    match state
        .mcp
        .call_tool(
            &state.db,
            &cfg.permissions.default_mcp,
            &call.server,
            &call.tool,
            call.arguments.clone(),
            skip_approval,
        )
        .await
    {
        Ok(result) => {
            *saw_tool_result = true;
            remember_zoho_from_call(state, session_id, &call);
            state
                .zoho_ctx
                .ingest(&state.db, session_id, Some(&call.server), &result);
            let body = truncate_result(&result);
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: {
                    // Keep history small: store the call fence only
                    if parse_tool_call(&reply).is_some() && reply.chars().count() > 400 {
                        format!(
                            "```mcp-call\n{}\n```",
                            serde_json::json!({
                                "server": call.server,
                                "tool": call.tool,
                                "arguments": call.arguments,
                            })
                        )
                    } else {
                        reply
                    }
                },
            });
            messages.push(ChatMessage {
                role: "user".into(),
                content: format!(
                    "TOOL RESULT from {}/{}:\n{}\n\n\
                     If you still need another tool to fully answer the user (e.g. you have \
                     portal_id but still need project_id or tasks), emit ONLY an mcp-call. \
                     Otherwise answer the user in clear Italian: list what matters \
                     (names, status, owners, due dates). Do not dump raw JSON.",
                    call.server, call.tool, body
                ),
            });
            Ok(OneCall::Continue(messages))
        }
        Err(e) if e.starts_with("APPROVAL_REQUIRED:") => {
            let _ = app.emit("chat-status", "");
            Ok(OneCall::Done(LoopOutcome::NeedsApproval {
                server: call.server,
                tool: call.tool,
                level: cfg.permissions.default_mcp.clone(),
            }))
        }
        Err(e) => {
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: reply,
            });
            messages.push(ChatMessage {
                role: "user".into(),
                content: format!(
                    "TOOL ERROR from {}/{}: {e}\n\
                     Fix arguments and emit another mcp-call, or explain briefly to the user.",
                    call.server, call.tool
                ),
            });
            Ok(OneCall::Continue(messages))
        }
    }
}

/// After user approval: execute the pending call and continue the agent loop
/// (further tools in this chain skip the permission prompt).
pub async fn resume_after_approval(
    app: &AppHandle,
    state: &AppState,
    cfg: &AppConfig,
    session_id: &str,
    tools: &[McpToolInfo],
    snap_system: String,
) -> Result<String, String> {
    let pending = state
        .mcp
        .take_pending()
        .ok_or_else(|| "Nessuna chiamata MCP in attesa.".to_string())?;

    let _ = app.emit(
        "chat-status",
        format!("MCP {}/{}…", pending.server, pending.tool),
    );
    crate::debuglog::info(
        Some(app),
        format!(
            "mcp-agent:approve_resume {}/{}",
            pending.server, pending.tool
        ),
    );

    let result = state
        .mcp
        .call_tool(
            &state.db,
            &cfg.permissions.default_mcp,
            &pending.server,
            &pending.tool,
            pending.arguments.clone(),
            true,
        )
        .await?;

    // Prefer a deterministic summary for Zoho list tools (local models struggle with big JSON).
    if let Some(answer) = format_zoho_list_answer(&pending.tool, &result) {
        let _ = app.emit("chat-status", "");
        // Persist ctx from the approved call args
        let call = ParsedCall {
            server: pending.server.clone(),
            tool: pending.tool.clone(),
            arguments: pending.arguments.clone(),
        };
        remember_zoho_from_call(state, session_id, &call);
        state
            .zoho_ctx
            .ingest(&state.db, session_id, Some(&pending.server), &result);
        crate::debuglog::info(
            Some(app),
            format!(
                "mcp-agent:approve_summary tool={} chars={}",
                pending.tool,
                answer.chars().count()
            ),
        );
        return Ok(answer);
    }

    let body = truncate_result(&result);
    {
        let call = ParsedCall {
            server: pending.server.clone(),
            tool: pending.tool.clone(),
            arguments: pending.arguments.clone(),
        };
        remember_zoho_from_call(state, session_id, &call);
        state
            .zoho_ctx
            .ingest(&state.db, session_id, Some(&pending.server), &result);
    }

    let history = crate::sessions::list_messages(&state.db, session_id)?;
    let mut messages = vec![ChatMessage {
        role: "system".into(),
        content: {
            let mut s = snap_system;
            s.push_str(&tools_system_block(tools));
            s
        },
    }];
    for m in history.iter().rev().take(16).collect::<Vec<_>>().into_iter().rev() {
        messages.push(ChatMessage {
            role: m.role.clone(),
            content: sanitize_history_content(&m.content),
        });
    }
    messages.push(ChatMessage {
        role: "assistant".into(),
        content: format!(
            "```mcp-call\n{}\n```",
            serde_json::json!({
                "server": pending.server,
                "tool": pending.tool,
                "arguments": pending.arguments,
            })
        ),
    });
    messages.push(ChatMessage {
        role: "user".into(),
        content: format!(
            "TOOL RESULT from {}/{} (user approved this call):\n{}\n\n\
             Continue the workflow: emit the next mcp-call if still needed to fully answer \
             the user's last request, otherwise give a clear Italian summary. \
             Further tool calls in this turn are pre-approved.",
            pending.server, pending.tool, body
        ),
    });

    match run_loop(app, state, cfg, messages, tools, session_id, true).await? {
        LoopOutcome::Answer(reply) => Ok(reply),
        LoopOutcome::NeedsApproval { server, tool, .. } => Ok(format!(
            "Serve un’altra approvazione per `{server}/{tool}`. Digita `approva mcp`."
        )),
    }
}

pub fn sanitize_history_content(content: &str) -> String {
    let c = content.trim();
    if c.starts_with("MCP ") && c.contains("eseguito:") {
        let cut: String = c.chars().take(500).collect();
        return format!("{cut}\n…[json omesso]");
    }
    if c.starts_with("TOOL RESULT from ") && c.chars().count() > MAX_RESULT_CHARS {
        let cut: String = c.chars().take(MAX_RESULT_CHARS).collect();
        return format!("{cut}\n…[truncated]");
    }
    if c.chars().count() > 5000 {
        let cut: String = c.chars().take(5000).collect();
        return format!("{cut}\n…[truncated]");
    }
    content.to_string()
}

fn strip_residual_fences(text: &str) -> String {
    if parse_tool_call(text).is_some() && !text.chars().any(|c| c == '\n') {
        return "Ho bisogno di un altro passaggio per usare gli strumenti. Riprova o digita `approva mcp` se c’è una richiesta in sospeso.".into();
    }
    // If the model mixed prose + call, prefer prose without the fence for final display
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?is)```\s*mcp(?:-call)?\s*\n.*?```").expect("strip fence")
    });
    let cleaned = re.replace_all(text, "").trim().to_string();
    if cleaned.is_empty() {
        return "Ho bisogno di un altro passaggio per usare gli strumenti. Riprova o digita `approva mcp` se c’è una richiesta in sospeso.".into();
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_call() {
        let text = r#"```mcp-call
{"server":"echo","tool":"echo","arguments":{"text":"hi"}}
```"#;
        let c = parse_tool_call(text).expect("parse");
        assert_eq!(c.server, "echo");
        assert_eq!(c.tool, "echo");
        assert_eq!(c.arguments["text"], "hi");
    }

    #[test]
    fn parses_xml_call() {
        let text = r#"<mcp_call server="cal" tool="list_events">{"days":1}</mcp_call>"#;
        let c = parse_tool_call(text).expect("parse");
        assert_eq!(c.server, "cal");
        assert_eq!(c.tool, "list_events");
    }

    #[test]
    fn no_call_in_prose() {
        assert!(parse_tool_call("Le tue riunioni sono alle 15.").is_none());
    }

    #[test]
    fn detects_plan_prose() {
        let text = "Per trovare i task devo prima ottenere il portal_id. Procedo con la sequenza: \
                    lista dei portali, poi progetti.";
        assert!(looks_like_plan_without_tool(text));
    }

    #[test]
    fn detects_broken_mcp_json() {
        let text = r#"{"server":"zoho-projects","tool":""#;
        assert!(looks_like_broken_mcp_call(text));
        assert!(looks_incomplete_answer(text));
    }

    #[test]
    fn deterministic_issues_from_ids() {
        let tools = vec![McpToolInfo {
            server: "zoho-projects".into(),
            name: "ZohoProjects_get_all_issues".into(),
            description: "issues".into(),
            input_schema: serde_json::Value::Null,
        }];
        let text = "portal_id è 20067605870 e project_id è 72448000005164077 issues aperti";
        let c = try_deterministic_zoho_call(&tools, text, "", None).expect("det");
        assert_eq!(c.tool, "ZohoProjects_get_all_issues");
        assert_eq!(c.arguments["path_variables"]["portal_id"], "20067605870");
        assert!(c.arguments["path_variables"].get("project_id").is_none());
        assert_eq!(c.arguments["query_params"]["page"], "1");
        assert!(c.arguments["query_params"]["filter"]
            .as_str()
            .unwrap_or("")
            .contains("72448000005164077"));
    }

    #[test]
    fn deterministic_issue_detail_uses_ctx() {
        let tools = vec![McpToolInfo {
            server: "zoho-projects".into(),
            name: "ZohoProjects_get_issue".into(),
            description: "one issue".into(),
            input_schema: serde_json::Value::Null,
        }];
        let ctx = crate::mcp::zoho_ctx::ZohoCtx {
            session_id: "s".into(),
            portal_id: Some("20067605870".into()),
            project_id: Some("72448000005164077".into()),
            entities: vec![],
        };
        let latest = "puoi darmi i dettagli di questo: 72448000009293006";
        let c = try_deterministic_zoho_call(&tools, latest, "", Some(&ctx)).expect("detail");
        assert_eq!(c.tool, "ZohoProjects_get_issue");
        assert_eq!(
            c.arguments["path_variables"]["issue_id"],
            "72448000009293006"
        );
        assert_eq!(
            c.arguments["path_variables"]["portal_id"],
            "20067605870"
        );
    }

    #[test]
    fn compact_uses_structured() {
        let v = json!({
            "content": [{"type":"text","text":"ignored"}],
            "structuredContent": {"data":{"result":[{"id":"1","name":"A"}]}}
        });
        let s = truncate_result(&v);
        assert!(s.contains("\"id\""));
        assert!(!s.contains("ignored"));
    }
}
