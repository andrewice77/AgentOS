# PRD — AgentOS (Personal Desktop Agent)

**Versione documento:** 2.0  
**Nome prodotto (working title):** AgentOS  
**Nome brand definitivo:** da scegliere (candidati: Lar, Familiar, Aegis, Sidecar, Hearth, Caret)  
**Tipo prodotto:** Local-first AI Personal Desktop Assistant  
**Target piattaforma v1:** Linux Desktop (Debian/Ubuntu; riferimento sviluppo: KDE + X11)  
**Privacy model:** Local-first con cloud LLM opt-in  

---

# 1. Visione del prodotto

Creare un assistente personale AI che vive sul computer dell'utente, con presenza grafica discreta sul desktop, capace di assistere nella vita quotidiana e lavorativa attraverso conversazioni naturali, memoria persistente ispirata a Hermes, strumenti controllati e integrazione MCP.

L'obiettivo non è un chatbot, ma un **compagno digitale personale** che:

- conosce l'utente
- ricorda informazioni importanti in modo curato e bounded
- aiuta nell'organizzazione
- esegue azioni autorizzate (built-in + MCP)
- evolve nel tempo (learning loop completo in v2)

Il prodotto deve essere percepito come un assistente personale simile a Siri, ma **local-first**, privato e personalizzabile.

---

# 2. Problema da risolvere

Gli strumenti AI attuali presentano diversi limiti:

- richiedono connessione cloud di default
- non hanno memoria personale persistente e controllabile
- non conoscono il contesto dell'utente tra sessioni
- non possono interagire realmente con il computer in modo sicuro
- trattano ogni conversazione come isolata

L'utente necessita di un assistente sempre disponibile che organizzi attività, supporti il lavoro, mantenga memoria delle decisioni e rispetti privacy e controllo personale.

---

# 3. Obiettivi principali

## Obiettivo primario (MVP / thin-slice)

Realizzare un assistente AI desktop Linux con:

- interfaccia tray + chat
- avatar semplice con stati visivi
- LLM locale (Ollama) + connettori cloud opt-in
- memoria Hermes-core (USER + MEMORY + session search)
- task / reminder / notifiche
- client MCP con permission gate
- action ledger e badge Locale/Cloud

## Obiettivi non-MVP (esplicitamente fuori dallo slice)

- Piper TTS / voice input
- RAG / Qdrant pesante
- floating avatar ultra-raffinato
- marketplace MCP
- Docker obbligatorio
- skills auto-learning (Hermes full loop)
- macOS / Windows packaging

---

# 4. Principi fondamentali

## Privacy First (local-first)

- Default: LLM locale (Ollama).
- Cloud LLM solo su opt-in esplicito (API key in OS keyring).
- Memoria, sessioni, task e file restano sempre sulla macchina.
- Badge di sessione sempre visibile: `Locale` oppure `Cloud · <provider>`.

## Human Control

L'agente non ha mai controllo totale del sistema. Operazioni sensibili richiedono conferma. Memory writes possono richiedere approval (default: on).

## Progressive Intelligence

Iniziare semplice, aumentare capacità nel tempo. Non consumare risorse inutilmente (target hardware: GPU 8GB class).

## Personal Growth

L'assistente migliora conoscendo preferenze, abitudini, progetti e decisioni — tramite memoria curata (v1) e skills/learning loop (v2).

---

# 5. Utente principale

Sviluppatore / operatore Linux professionista che lavora con codice, server, database e gestione progetti. L'assistente è utile sia fuori dal lavoro sia durante lo sviluppo.

**Hardware di riferimento (dev):** Quadro P4000 8GB VRAM, ~32GB RAM, Xeon W-2235, X11 + KDE.

---

# 6. User Experience desiderata

## Naturale

> "Organizzami la giornata"  
> "Ricordami domani di controllare il server"  
> "Cosa avevamo deciso sul progetto?"  
> "Usa il server MCP X per …"

## Presente ma non invasiva

- icona tray
- avatar 2D con stati
- notifiche discrete
- apertura da click tray o shortcut

---

# 7. Funzionalità MVP (Versione 1 — thin-slice)

## 7.1 Desktop shell

- icona nella system tray
- finestra chat principale
- avatar semplice con stati: Idle, Thinking, Speaking, Notification, Error

## 7.2 Conversazione AI

- messaggi testo + risposte streaming
- contesto conversazionale
- router LLM: Ollama (default) + OpenAI / Anthropic (opt-in)

## 7.3 Memoria Hermes-core

Pattern ispirato a Hermes Agent (non un fork):

| Layer | Ruolo |
|-------|--------|
| **USER** | Profilo utente (preferenze, stile, identità) — bounded |
| **MEMORY** | Note agente (ambiente, progetti, lezioni) — bounded |
| **Sessions + FTS5** | Archivio conversazioni illimitato, recall on-demand |

Comportamento:

- snapshot frozen di USER+MEMORY injectato a inizio sessione
- tool `memory` (add / replace / remove) gestito dall'agente
- limiti caratteri espliciti + consolidamento quando pieni
- `write_approval` default **on**
- tool `session_search` su SQLite FTS5
- UI "Cosa sa di me" (view / edit / delete)
- scan base anti prompt-injection sulle entry

## 7.4 Organizer

- task (create / update / complete)
- reminder con scheduling
- notifiche desktop

## 7.5 MCP client

- connessione a 0–N server MCP via **stdio** o **HTTP** (streamable / SSE)
- OAuth 2 / PKCE generico (token nel keyring), non legato a un solo vendor
- tool MCP soggetti al Permission Manager
- watch configurabili: polling di qualsiasi tool → notifica su cambiamento o orario imminente
- nessun marketplace in v1; preset UI (Gmail, Calendar) solo come scorciatoia di configurazione

## 7.6 Sicurezza operativa

Livelli:

| Livello | Comportamento |
|---------|----------------|
| SAFE | Automatico (chat, lettura memoria, reminder) |
| READ | Config iniziale / whitelist |
| OPERATE | Conferma utente |
| ADMIN | Sempre conferma manuale |

Action ledger: ogni proposta/esecuzione tool registrata in modo umano-leggibile.

## 7.7 Suggerimenti

Solo eventi espliciti (es. briefing opzionale all'avvio). Niente osservazione continua del comportamento.

---

# 8. Funzioni future

## Versione 2

- Voice output (Piper) e voice input (Whisper.cpp)
- Skills procedurali (agentskills.io) + background self-improvement review
- Journey / memory graph
- Floating avatar raffinato
- Tool filesystem / git / terminale sandbox (oltre MCP)
- Calendario CalDAV / integrazioni reali
- Docker profile per modalità server/lab

## Versione 3

- Assistente proattivo (pattern, anticipazione)
- Automazioni avanzate
- Packaging macOS / Windows
- Provider memoria esterni opzionali (Mem0 / Honcho-style)

---

# 9. Vincoli tecnici

| Vincolo | Dettaglio |
|---------|-----------|
| OS v1 | Linux desktop first |
| Runtime | Agent-core **embedded** in Tauri/Rust (un solo processo app) |
| LLM locale | Ollama, modelli ~7–8B Q4/Q5, context 8k–16k |
| Cloud | Opt-in; stesse API OpenAI-compatible dove possibile |
| Storage | SQLite (+ FTS5); niente Qdrant obbligatorio in v1 |
| Docker | Non obbligatorio in v1 |
| Stack UI | Svelte 5 + TypeScript + Vite |
| Shell | Tauri 2 + Rust |

---

# 10. Metriche di successo

Il progetto è riuscito quando l'utente può dire: *"Questo assistente mi fa risparmiare tempo ogni giorno."*

Indicatori:

- utilizzo giornaliero
- task/reminder gestiti
- qualità memoria (entry utili, poche correzioni)
- tool MCP usati con successo sotto approval
- basso consumo risorse su GPU 8GB

---

# 11. Roadmap

## Milestone 1 — Foundation

- app Tauri + Svelte
- tray + chat streaming
- Ollama + cloud opt-in stub
- memoria Hermes-core
- task/reminder
- MCP client + permissions + ledger

## Milestone 2 — Personal Assistant+ (current)

- TTS (Piper + Web Speech), STT Whisper opzionale
- avatar floating + skin picker
- briefing mattutino raffinato
- skills + background review
- packaging macOS/Windows **rimandato** a Milestone 4

## Milestone 3 — Computer Assistant

- tool nativi developer, sandbox, integrazioni calendario

## Milestone 4 — Intelligent Companion

- proattività, automazioni, multi-OS (packaging macOS/Windows)

---

# 12. Criteri di accettazione MVP

- [x] L'app parte su Linux (Debian-class) con tray
- [x] Chat streaming con Ollama funziona
- [x] Cloud provider configurabile in opt-in con badge visibile
- [x] Memoria USER/MEMORY persistente + UI di revisione
- [x] Session search FTS5 operativo
- [x] Task e reminder funzionanti con notifica
- [x] Almeno un server MCP configurabile ed eseguibile sotto permission gate
- [x] Action ledger consultabile
- [x] Nessun dato inviato a cloud senza opt-in esplicito
