# SRS — AgentOS (Personal Desktop Agent)

**Versione:** 2.0  
**Stato:** Specifica tecnica allineata alle fondamenta prodotto  
**Piattaforma target v1:** Linux Desktop (Debian/Ubuntu)  
**Architettura:** Local-first AI Agent System, core embedded in Tauri

---

# 1. Introduzione

## 1.1 Scopo

Requisiti software per un assistente AI desktop locale con:

- shell Tauri 2 + UI Svelte 5
- agent runtime embedded (Rust)
- LLM locale (Ollama) + cloud opt-in
- memoria Hermes-core
- organizer (task/reminder)
- client MCP + permission manager
- action ledger

Il sistema funziona senza servizi cloud obbligatori.

---

# 2. Architettura generale

```
                    USER
                     |
             Tauri Desktop App
             (Rust shell + Svelte UI)
                     |
              Agent Runtime (embedded)
                     |
        ---------------------------------
        |        |         |            |
       LLM    Memory    Permissions    MCP
      Router  Hermes     Manager      Client
        |        |                      |
   Ollama/Cloud  SQLite+FTS5      stdio servers
```

### Decisioni architetturali

| Decisione | Scelta v1 |
|-----------|-----------|
| Agent core | Embedded nel processo Tauri (non servizio Docker obbligatorio) |
| UI | Svelte 5 + TypeScript + Vite |
| Backend | Rust |
| Vector DB | Non richiesto in v1 |
| Agent frameworks esterni | Non dipendenza (OpenClaw/Hermes = ispirazione) |

---

# 3. Componenti principali

1. Desktop Client (Tauri + Svelte)
2. Agent Runtime
3. LLM Router
4. Memory System (Hermes-core)
5. Tool System (built-in + MCP)
6. Permission Manager
7. Scheduler / Organizer
8. Action Ledger
9. Config & Data directory

---

# 4. Desktop Client

## Tecnologia

```
Tauri 2 + Rust
Svelte 5 + TypeScript + Vite
```

## Responsabilità

- tray icon e menu
- finestra chat
- avatar e stati
- badge Locale/Cloud
- UI memoria ("Cosa sa di me")
- UI approval (memory writes, tool OPERATE/ADMIN)
- UI config MCP / provider LLM
- notifiche desktop
- action ledger view

## Stati avatar

```
IDLE | THINKING | SPEAKING | NOTIFICATION | ERROR
```

## Comunicazione

IPC Tauri commands + eventi (streaming token, tool approval requests, memory pending).

Niente API HTTP pubblica in v1. Bind esterni vietati.

---

# 5. Agent Runtime

## Responsabilità

- orchestrazione turn
- injection memoria frozen
- tool calling (built-in + MCP)
- aggiornamento sessioni
- richieste di approval

## Agent loop

```
User Input
  → Persist user message
  → Build prompt (system + frozen USER/MEMORY + recent turns)
  → LLM (stream)
  → Optional tool calls → Permission Manager → execute / deny
  → Persist assistant message
  → Optional memory tool side-effects (gated by write_approval)
  → Output to UI
```

---

# 6. LLM Layer

## Provider

| Provider | Tipo | Note |
|----------|------|------|
| Ollama | Locale (default) | OpenAI-compatible `localhost:11434` |
| OpenAI | Cloud opt-in | API key in keyring |
| Anthropic | Cloud opt-in | API key in keyring |

## Model Manager

- lista modelli disponibili (Ollama)
- selezione modello attivo
- flag provider attivo + badge UI
- default consigliato: Qwen 7B/8B family Q4/Q5, context 8k–16k

## Requisiti

- streaming token obbligatorio per chat
- tool/function calling dove supportato dal provider
- nessun invio cloud se provider = local

---

# 7. Memory System (Hermes-core)

## 7.1 Hot memory (bounded)

Due store:

### user_profile (USER)

Preferenze, identità, stile comunicazione.  
Limite default: **1375** caratteri.

### agent_memory (MEMORY)

Fatti ambiente, progetti, lezioni.  
Limite default: **2200** caratteri.

Persistenza: tabelle SQLite (e/o file markdown di export sotto `~/.agentos/memory/`).

Injection: **frozen snapshot** a inizio sessione (non aggiornato mid-session nel system prompt; tool response mostra stato live).

### Tool `memory`

Actions: `add` | `replace` | `remove`  
Targets: `user` | `memory`

Se overflow → errore con entry correnti; l'agente consolida e riprova.

### write_approval

Default: `true`.  
Write → staged → UI approve/reject → commit.

### Security

Scan entry per pattern injection / exfiltration prima del commit.

## 7.2 Cold recall

- tabella `messages` + virtual table FTS5
- tool `session_search` (query, scroll, browse)
- nessun hard cap sullo storico sessioni

## 7.3 Working memory

Contesto turn corrente e task attivi in SQLite / stato processo.

## 7.4 Fuori scope v1

- Skills agentskills.io
- Background self-improvement review
- Journey / memory graph
- External providers (Mem0, Honcho, …)
- Qdrant / RAG documentale pesante

---

# 8. Tool System

## Built-in MVP

| Tool | Permission | Descrizione |
|------|------------|-------------|
| `memory` | SAFE (+ approval write) | Gestione memoria curata |
| `session_search` | SAFE | Ricerca sessioni FTS5 |
| `web_search` | READ | DuckDuckGo + fetch 1–2 pagine (`cerca online:`); fallback Wikipedia/news |
| `task` | SAFE | CRUD task |
| `reminder` | SAFE | Crea/modifica reminder |

## MCP

Ogni server MCP in config:

```yaml
mcp_servers:
  - name: filesystem
    transport: stdio
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]
    enabled: true
  - name: gmail
    transport: http
    url: https://gmailmcp.googleapis.com/mcp/v1
    oauth:
      client_id: "..."
      client_secret: "..."
      auth_url: https://accounts.google.com/o/oauth2/v2/auth
      token_url: https://oauth2.googleapis.com/token
      scopes: ["https://www.googleapis.com/auth/gmail.readonly"]
    watches:
      - name: inbox
        tool_hint: message
        interval_secs: 300
        title: Nuove email
        notify_on_change: true
    enabled: true
```

Regole:

- discovery tools a connect (stdio con framing Content-Length o NDJSON; HTTP JSON o SSE)
- ogni tool call MCP passa da Permission Manager
- **agent loop generico**: se ci sono tool connessi, il modello può emettere `mcp-call` e AgentOS esegue qualsiasi tool scoperto (nessuna logica tipizzata su mail/finance/…)
- OAuth opzionale per-server; token in keyring
- watch: polling periodico di un tool, notifica desktop su hash diverso o datetime imminente
- default mapping: READ per tool di sola lettura dichiarati; OPERATE altrimenti (override in config)
- disabilitabile per-server e per-tool

---

# 9. Permission Manager

## Levels

```
SAFE | READ | OPERATE | ADMIN
```

## Comportamento

- SAFE: esegui
- READ: richiede abilitazione path/server in config
- OPERATE / ADMIN: richiesta conferma UI con payload leggibile
- deny → tool result di errore controllato all'agente

---

# 10. Scheduler / Organizer

## Tasks

Campi: id, title, description, status, priority, deadline, created_at, updated_at

## Reminders / Events

Campi: id, type, schedule (datetime), action/message, enabled

## Notifiche

Notifica desktop nativa (Tauri notification plugin) allo scatto reminder.

---

# 11. Voice System

**v2:** TTS configurabile in Settings (`voice` in `config.yaml`).

- Motore `system`: Web Speech API (voci OS)
- Motore `piper`: Piper locale (`~/.agentos/voice/piper/`, setup via `scripts/setup-voice.sh`)
- Motore `audio8`: [Audio8 TTS 0.6B](https://huggingface.co/Audio8/Audio8-TTS-Preview-0.6b)
  - default **CPU** via ONNX service (`scripts/setup-audio8.sh`, porta 8024) — lascia la GPU a Ollama
  - opzionale **CUDA** via `scripts/audio8_infer_gpu.py` (compete in VRAM con Ollama)
- Auto-read risposte assistente (toggle)
- STT opzionale via Whisper.cpp + `arecord`/`rec`

**Fuori scope attuale:** packaging binari Piper/Whisper nell’installer macOS/Windows.

---

# 12. Action Ledger

Ogni evento tool registra:

```
timestamp | actor | tool | permission | status | summary | detail_json
```

Status: `proposed` | `approved` | `denied` | `executed` | `failed`

UI: lista filtrabile.

---

# 13. Database Design

Database: SQLite in `~/.agentos/database.sqlite`

## Schema iniziale

### meta

```
key TEXT PRIMARY KEY
value TEXT
```

### memories

```
id INTEGER PK
store TEXT CHECK(store IN ('user','memory'))
content TEXT NOT NULL
created_at TEXT
updated_at TEXT
```

### memory_pending

```
id INTEGER PK
store TEXT
action TEXT
content TEXT
old_text TEXT
created_at TEXT
```

### conversations / sessions

```
sessions(id, title, created_at, updated_at, provider, model)
messages(id, session_id, role, content, created_at)
messages_fts(FTS5 over content)
```

### tasks

```
id, title, description, status, priority, deadline, created_at, updated_at
```

### events

```
id, type, schedule, message, enabled, created_at
```

### action_ledger

```
id, timestamp, tool, permission, status, summary, detail_json
```

### mcp_servers (opzionale DB o solo config.yaml)

Preferenza v1: config.yaml; stato connessione in memoria processo.

---

# 14. Configurazione

Directory dati:

```
~/.agentos/
  config.yaml
  database.sqlite
  memory/
  logs/
  avatar/
```

## config.yaml (esempio)

```yaml
provider: ollama          # ollama | openai | anthropic
model: qwen2.5:7b
ollama:
  base_url: http://127.0.0.1:11434
openai:
  base_url: https://api.openai.com/v1
anthropic:
  base_url: https://api.anthropic.com
memory:
  user_char_limit: 1375
  memory_char_limit: 2200
  write_approval: true
mcp_servers: []
permissions:
  default_mcp: operate
```

Segreti: OS keyring (non in plaintext nel yaml).

---

# 15. Sicurezza

- nessun bind pubblico
- nessun invio cloud senza opt-in
- directory/tool whitelist dove applicabile
- memory injection scanning
- log azioni agente (ledger)
- conferma UI per OPERATE/ADMIN

---

# 16. Logging

File sotto `~/.agentos/logs/`:

```
timestamp | level | component | message
```

Separato dal ledger utente (che è product-facing).

---

# 17. Repository Structure

```
agentos/
├── docs/
│   ├── PRD.md
│   └── SRS.md
├── src/                    # Svelte frontend
│   ├── lib/
│   ├── routes/ or App
│   └── styles/
├── src-tauri/              # Rust / Tauri
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── agent/
│   │   ├── memory/
│   │   ├── llm/
│   │   ├── mcp/
│   │   ├── permissions/
│   │   ├── organizer/
│   │   └── db/
│   ├── Cargo.toml
│   └── tauri.conf.json
├── package.json
├── README.md
└── ...
```

---

# 18. Docker

**Non richiesto per v1 desktop.**  
Futuro: profilo compose per agent-core in modalità server/lab.

---

# 19. MVP Development Tasks

## Sprint A — Shell

- scaffold Tauri 2 + Svelte 5
- tray + finestra chat
- stati avatar base
- data dir `~/.agentos`

## Sprint B — LLM + chat

- Ollama streaming
- cloud provider stubs opt-in
- badge provider
- persistenza sessioni

## Sprint C — Memory

- USER/MEMORY stores + tool
- write_approval UI
- FTS5 session_search
- UI "Cosa sa di me"

## Sprint D — Organizer + MCP + security

- task/reminder/scheduler
- MCP stdio client
- permission manager
- action ledger

---

# 20. Criteri di accettazione tecnici MVP

- [x] Build/run su Linux con `npm run tauri dev` (o equivalente release)
- [x] Chat streaming Ollama end-to-end
- [x] Switch provider cloud con keyring + badge
- [x] Memory add/replace/remove con limiti e approval
- [x] session_search restituisce hit FTS5
- [x] Task/reminder + notifica
- [x] MCP server stdio configurabile; tool call gated
- [x] Ledger popolato per ogni tool call
- [x] Nessuna telemetria / nessun cloud implicito
