# AgentOS

Local-first personal desktop AI agent for Linux (**Tauri 2 + Rust + Svelte 5**).

**In costruzione.** Il progetto è attivo e incompleto: API, UI e brand possono ancora cambiare. Consigli, issue e collaborazioni sono i benvenuti.

This project is under active construction. Feedback, ideas, and pull requests are welcome.

Repo: [github.com/andrewice77/AgentOS](https://github.com/andrewice77/AgentOS)

Working title: **AgentOS** — brand definitivo ancora da scegliere (Lar / Familiar / Aegis / Sidecar / Hearth / Caret).

## MVP incluso

| Area | Stato |
|------|--------|
| Tray + finestra chat | OK |
| Streaming Ollama (`think: false` + fallback) | OK |
| Cloud opt-in OpenAI/Anthropic + keyring + badge | OK |
| Memoria USER/MEMORY + approval + FTS5 | OK |
| Task / reminder NL + notifiche desktop | OK |
| MCP stdio + permission gate + ledger | OK |
| Server smoke `scripts/mcp_echo.py` | OK |

## Milestone 2 (v2) — Personal Assistant+

| Area | Stato |
|------|--------|
| TTS (Web Speech + Piper locale) | OK — Settings → Voce |
| Audio8 0.6B TTS (CPU ONNX / CUDA opz.) | OK — `scripts/setup-audio8.sh` |
| Lettura automatica risposte | OK — toggle in Settings |
| STT Whisper.cpp (opzionale) | OK — richiede binario + modello |
| Briefing mattutino | OK — Settings → Briefing |
| Skills procedurali + background review | OK — tab Skills |
| Packaging macOS / Windows | **Rimandato** (Milestone 4) |

### Setup voce (Linux)

```bash
./scripts/setup-voice.sh            # Piper + voce italiana
# Opzionale Whisper: metti whisper-cli + ggml-*.bin in ~/.agentos/voice/whisper/
```

Poi in **Settings → Voce**: abilita TTS, scegli motore `system` (subito) o `piper`, Salva, **Prova TTS**.

### Audio8 (qualità alta)

Con Ollama sulla GPU usa **CPU ONNX** (non compete sulla VRAM):

```bash
./scripts/setup-audio8.sh
cd ~/.agentos/voice/audio8/Audio8_TTS/onnx_runtime && bash start_server.sh
# registra una voce su http://127.0.0.1:8024
```

Settings → Motore **audio8** → Device **cpu** → Voce Audio8 = nome registrato.

Device **cuda** è opzionale e condivide la GPU con Ollama (può andare in OOM).

## Prerequisiti (Debian/Ubuntu)

```bash
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf pkg-config python3

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Ollama + modello locale (~7–9B Q4 su 8GB VRAM)
# https://ollama.com
# Esempio già usato in sviluppo:
#   ollama create qwen3.5:9b-q4km -f models/Modelfile.qwen35-9b
```

## Build & avvio (consigliato)

Su macchine con Docker/virtiofs, preferisci **release** a `tauri dev`.

**Sviluppo locale (consigliato):**

```bash
cd /path/to/agentos
make          # build UI+Rust e avvia companion (avatar floating)
make kill     # chiude l'istanza
make restart  # kill + rebuild + avvia
make help
```

Il `Makefile` è la fonte di verità per lo sviluppo: aggiornalo se cambi build/env/avvio.

Oppure in un colpo solo via npm:

```bash
npm install
npm run start:release
```

Passo-passo:

```bash
npm install
npm run build
cd src-tauri
WEBKIT_DISABLE_DMABUF_RENDERER=1 cargo run --release
```

Dati locali: `~/.agentos/` (`config.yaml`, `database.sqlite`, `logs/`).

### NVIDIA / WebKit blank window

Se la finestra è nera o vedi errori GBM/DRM:

```bash
export WEBKIT_DISABLE_DMABUF_RENDERER=1
```

AgentOS lo imposta già all’avvio su Linux.

## Smoke test

```bash
npm run smoke
```

Verifica: Ollama raggiungibile, MCP echo stdio, unit test Rust (parser reminder / FTS), binario release presente.

## Comandi chat (built-in)

| Comando | Effetto |
|---------|---------|
| `ricorda …` / `preferisco …` | Memoria (con approval se attivo) |
| `cosa sai di me` | Dump USER/MEMORY |
| `dimentica …` | Rimuove entry |
| `approva memoria N` | Approva pending |
| `cerca: …` | Session search FTS/LIKE |
| `cerca online: …` | Ricerca DuckDuckGo (+ Wikipedia/news se DDG è bloccato) e riassunto in chat |
| `task: …` / `lista task` / `completa task N` | Organizer |
| `ricordami tra 2 minuti di …` | Reminder NL |
| `ricordami domani alle 9 di …` | Reminder NL |
| `mcp list` / `mcp call …` / `approva mcp` | MCP (manuale) |

Con server MCP collegati, in chat basta linguaggio naturale: AgentOS sceglie e chiama i tool da solo (generico: mail, calendar, finance, marketing, …).

## MCP smoke

1. Tab **MCP** → **Installa echo smoke** → **Riconnetti**
2. Esegui tool `echo` (con permission `operate` serve **Approva**)
3. Controlla **Ledger**

Per un server HTTP (Gmail, Calendar, o qualsiasi altro MCP remoto): tab **MCP** → preset o URL custom → **Salva server** → **Accedi** (OAuth) → **Riconnetti**. I **watch** interrogano un tool a intervallo e mandano una notifica desktop se il risultato cambia o se c’è un orario imminente.

## Docs

- [docs/PRD.md](docs/PRD.md)
- [docs/SRS.md](docs/SRS.md)

## Fuori scope (rimandato)

Packaging macOS/Windows (Milestone 4), marketplace MCP, Docker obbligatorio, tool filesystem/git sandbox nativi (Milestone 3).

## Contribuire

Se vuoi dare una mano:

- apri una [issue](https://github.com/andrewice77/AgentOS/issues) per bug, idee o feedback
- manda una [pull request](https://github.com/andrewice77/AgentOS/pulls) per contributi di codice

Non serve un piano formale: anche un consiglio in una issue è utile.

## License

[MIT](LICENSE)
