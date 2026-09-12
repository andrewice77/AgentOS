<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Avatar2D from "$lib/components/Avatar2D.svelte";
  import MarkdownBody from "$lib/components/MarkdownBody.svelte";
  import PageHeader from "$lib/components/PageHeader.svelte";
  import OrganizerPanel from "$lib/components/OrganizerPanel.svelte";
  import McpPanel from "$lib/components/McpPanel.svelte";
  import type { AvatarState } from "$lib/avatar";
  import { AVATAR_SKINS, normalizeAvatarSkin } from "$lib/avatar";
  import { speakAssistantReply, startReadingReply, stopSpeaking } from "$lib/voice";
  import {
    ACCENT_PRESETS,
    applyUiSettings,
    DEFAULT_UI,
    normalizeUi,
    THEME_PRESETS,
    type UiSettings,
  } from "$lib/theme";

  type Tab = "chat" | "memory" | "tasks" | "mcp" | "ledger" | "settings" | "skills" | "debug";
  type SettingsSection = "appearance" | "avatar" | "provider" | "voice" | "briefing" | "skills";

  let tab = $state<Tab>("chat");
  let settingsSection = $state<SettingsSection>("appearance");
  let contentLayout = $state<"single" | "split">("split");
  let badge = $state("Locale · Ollama");
  let avatarState = $state<AvatarState>("idle");
  let avatarSkin = $state("buddy");
  let input = $state("");
  let streaming = $state("");
  let busy = $state(false);
  let chatStatus = $state("");
  let sessionId = $state<string | null>(null);
  let messages = $state<{ role: string; content: string }[]>([]);
  let error = $state("");
  let sessions = $state<{ id: string; title: string; updated_at: string }[]>([]);
  let showSessions = $state(true);

  let memory = $state<any>(null);
  let pending = $state<any[]>([]);
  let tasks = $state<any[]>([]);
  let reminders = $state<any[]>([]);
  let labels = $state<any[]>([]);
  let ledger = $state<any[]>([]);
  let mcpServers = $state<any[]>([]);
  let mcpTools = $state<any[]>([]);
  let config = $state<any>(null);
  let models = $state<string[]>([]);
  let apiKeyDraft = $state("");
  let searchQ = $state("");
  let searchHits = $state<any[]>([]);
  let memDraft = $state("");
  let memStore = $state<"user" | "memory">("memory");
  let memStatus = $state("");

  let mcpStatus = $state<any>(null);
  let mcpResult = $state("");
  let mcpPending = $state<any>(null);
  let pendingQuestion = $state<{
    id: string;
    question: string;
    options: string[];
    allow_free_text?: boolean;
    allowFreeText?: boolean;
  } | null>(null);
  let mcpPanelRef = $state<{ doReconnect?: () => Promise<void> } | null>(null);

  let debugLines = $state<{ ts: string; level: string; message: string }[]>([]);
  let diagnostics = $state<any>(null);
  let diagBusy = $state(false);
  let probeBusy = $state(false);
  let copyStatus = $state("");

  let voiceStatus = $state<any>(null);
  let briefing = $state<any>(null);
  let briefingDismissed = $state(false);
  let listening = $state(false);
  let skills = $state<any[]>([]);
  let skillDraft = $state("");
  let skillId = $state("");
  let skillReview = $state<any>(null);
  let skillBusy = $state(false);
  let skillMsg = $state("");
  let settingsMsg = $state("");
  let settingsSaving = $state(false);

  const NAV_GROUPS: { label: string; items: { id: Tab; label: string; icon: string; hint: string }[] }[] = [
    {
      label: "Principale",
      items: [{ id: "chat", label: "Chat", icon: "💬", hint: "Conversazione con AgentOS" }],
    },
    {
      label: "Organizzazione",
      items: [
        { id: "memory", label: "Memoria", icon: "🧠", hint: "Cosa sa di te" },
        { id: "tasks", label: "Organizer", icon: "📋", hint: "Task e promemoria" },
      ],
    },
    {
      label: "Sistema",
      items: [
        { id: "mcp", label: "MCP", icon: "🔌", hint: "Strumenti esterni" },
        { id: "ledger", label: "Ledger", icon: "📜", hint: "Cronologia azioni" },
        { id: "skills", label: "Skills", icon: "⚡", hint: "Competenze procedurali" },
      ],
    },
    {
      label: "Configurazione",
      items: [
        { id: "settings", label: "Impostazioni", icon: "⚙️", hint: "Aspetto e preferenze" },
        { id: "debug", label: "Debug", icon: "🛠", hint: "Diagnostica tecnica" },
      ],
    },
  ];

  const TAB_META: Record<Tab, { title: string; description: string; breadcrumb: string }> = {
    chat: {
      title: "Conversazione",
      description: "Scrivi qui per parlare con AgentOS. Usa il microfono se abilitato in Impostazioni → Voce.",
      breadcrumb: "Home / Chat",
    },
    memory: {
      title: "Memoria",
      description: "Informazioni che AgentOS ricorda su di te. Le entry USER definiscono il profilo; MEMORY le note libere.",
      breadcrumb: "Home / Memoria",
    },
    tasks: {
      title: "Organizer",
      description: "Bacheca a colonne, focus di oggi e agenda promemoria. Cattura anche in chat.",
      breadcrumb: "Home / Organizer",
    },
    mcp: {
      title: "MCP",
      description: "Collega qualsiasi server MCP. In chat AgentOS sceglie i tool da solo (mail, finance, marketing, …): niente personalizzazioni per dominio.",
      breadcrumb: "Home / MCP",
    },
    ledger: {
      title: "Action ledger",
      description: "Registro di tutte le azioni eseguite da AgentOS — utile per verificare cosa è successo.",
      breadcrumb: "Home / Ledger",
    },
    skills: {
      title: "Skills procedurali",
      description: "Istruzioni riutilizzabili che AgentOS applica automaticamente nelle conversazioni.",
      breadcrumb: "Home / Skills",
    },
    settings: {
      title: "Impostazioni",
      description: "Personalizza aspetto, avatar, provider AI, voce e altre preferenze.",
      breadcrumb: "Home / Impostazioni",
    },
    debug: {
      title: "Debug",
      description: "Strumenti diagnostici per operatori avanzati. Usa con cautela.",
      breadcrumb: "Home / Debug",
    },
  };

  const SETTINGS_SECTIONS: { id: SettingsSection; label: string; icon: string; blurb: string }[] = [
    { id: "appearance", label: "Aspetto", icon: "🎨", blurb: "Tema, colori e layout" },
    { id: "avatar", label: "Avatar", icon: "🤖", blurb: "Skin, dimensione e movimento" },
    { id: "provider", label: "Provider AI", icon: "🧩", blurb: "Modello e connessione" },
    { id: "voice", label: "Voce", icon: "🔊", blurb: "Lettura e microfono" },
    { id: "briefing", label: "Briefing", icon: "☀️", blurb: "Riassunto mattutino" },
    { id: "skills", label: "Skills", icon: "⚡", blurb: "Comportamento skills" },
  ];

  function ensureUiDefaults() {
    if (!config) return;
    if (!config.ui) {
      config.ui = { ...DEFAULT_UI };
    } else {
      config.ui = normalizeUi(config.ui);
    }
    contentLayout = config.ui.content_layout;
    applyUiSettings(config.ui as UiSettings);
  }

  function previewUi() {
    if (!config?.ui) return;
    applyUiSettings(normalizeUi(config.ui));
    contentLayout = config.ui.content_layout;
  }

  async function refreshBootstrap() {
    const data = await invoke<any>("get_bootstrap");
    badge = data.badge;
    memory = data.memory;
    pending = data.pending_memory ?? [];
    mcpPending = data.pending_mcp ?? null;
    pendingQuestion = data.pending_question ?? null;
    tasks = data.tasks ?? [];
    reminders = data.reminders ?? [];
    labels = data.labels ?? [];
    ledger = data.ledger ?? [];
    mcpServers = data.mcp_servers ?? [];
    try {
      mcpStatus = await invoke("mcp_status");
    } catch {
      /* stato MCP best-effort: la lista configurata resta in mcpServers */
    }
    config = data.config;
    if (config && config.avatar_roam == null) config.avatar_roam = false;
    ensureUiDefaults();
    avatarSkin = normalizeAvatarSkin(data.config?.avatar);
    sessionId = data.active_session;
    sessions = data.sessions ?? [];
    if (!data.ollama_reachable && config?.provider === "ollama") {
      error = "Ollama non raggiungibile su " + config.ollama.base_url;
    } else {
      error = "";
    }
    if (sessionId) {
      const rows = await invoke<any[]>("list_messages", { sessionId });
      messages = rows.map((r) => ({ role: r.role, content: r.content }));
    }
  }

  async function ensureVoiceDefaults() {
    if (!config) return;
    if (!config.voice) {
      config.voice = {
        enabled: false,
        auto_read_replies: true,
        engine: "system",
        voice: "it_IT-riccardo-x_low",
        rate: 1,
        volume: 1,
        piper_bin: "",
        piper_data_dir: "",
        stt_enabled: false,
        whisper_bin: "",
        whisper_model: "",
        audio8_base_url: "http://127.0.0.1:8024",
        audio8_device: "cpu",
        audio8_voice: "",
        audio8_model_id: "Audio8/Audio8-TTS-Preview-0.6b",
        audio8_runtime_dir: "",
        stream_sentences: true,
      };
    } else {
      config.voice.audio8_base_url ??= "http://127.0.0.1:8024";
      config.voice.audio8_device ??= "cpu";
      config.voice.audio8_voice ??= "";
      config.voice.audio8_model_id ??= "Audio8/Audio8-TTS-Preview-0.6b";
      config.voice.audio8_runtime_dir ??= "";
      config.voice.stream_sentences ??= true;
    }
    if (!config.briefing) {
      config.briefing = { enabled: false, hour: 8, llm_polish: false };
    }
    if (!config.skills) {
      config.skills = { enabled: true, inject_prompt: true };
    }
  }

  async function loadBriefing() {
    try {
      const offer = await invoke<boolean>("briefing_should_offer");
      if (!offer || briefingDismissed) return;
      briefing = await invoke("get_morning_briefing");
    } catch {
      briefing = null;
    }
  }

  async function refreshSkills() {
    skills = await invoke<any[]>("list_skills").catch(() => []);
    skillReview = await invoke("latest_skills_review").catch(() => null);
  }

  onMount(() => {
    let unsubs: (() => void)[] = [];
    (async () => {
      try {
        await refreshBootstrap();
        await ensureVoiceDefaults();
        models = await invoke<string[]>("list_local_models").catch(() => []);
        voiceStatus = await invoke("voice_status").catch(() => null);
        await loadBriefing();
        await refreshSkills();
      } catch (e) {
        error = String(e);
      }

      unsubs.push(
        await listen<string>("chat-status", (ev) => {
          chatStatus = ev.payload ?? "";
        }),
      );
      unsubs.push(
        await listen<string>("chat-token", (ev) => {
          streaming += ev.payload;
          if (avatarState === "thinking" || avatarState === "idle") {
            avatarState = "speaking";
          }
        }),
      );
      unsubs.push(
        await listen<string>("avatar-state", (ev) => {
          if (avatarState === "notification" && ev.payload !== "notification") return;
          avatarState = ev.payload as AvatarState;
        }),
      );
      unsubs.push(
        await listen<{ ts: string; level: string; message: string }>("debug-log", (ev) => {
          debugLines = [...debugLines, ev.payload].slice(-300);
        }),
      );
      unsubs.push(
        await listen<any>("reminder-due", async (ev) => {
          avatarState = "notification";
          reminders = await invoke<any[]>("list_reminders").catch(() => reminders);
          debugLines = [
            ...debugLines,
            {
              ts: String(Math.floor(Date.now() / 1000)),
              level: "info",
              message: `ui:reminder_due ${ev.payload?.message ?? ""}`,
            },
          ].slice(-300);
        }),
      );
      unsubs.push(
        await listen("reminder-acked", () => {
          if (avatarState === "notification") avatarState = "idle";
        }),
      );
      unsubs.push(
        await listen("reminders-changed", async () => {
          reminders = await invoke<any[]>("list_reminders").catch(() => reminders);
        }),
      );
      unsubs.push(
        await listen("tasks-changed", async () => {
          await refreshOrganizer();
        }),
      );
      unsubs.push(
        await listen("labels-changed", async () => {
          await refreshOrganizer();
        }),
      );
      unsubs.push(
        await listen("memory-changed", async () => {
          pending = await invoke<any[]>("list_pending_memory").catch(() => pending);
          memory = await invoke("get_memory").catch(() => memory);
        }),
      );
      unsubs.push(
        await listen<any>("mcp-pending", (ev) => {
          mcpPending = ev.payload ?? null;
        }),
      );
      unsubs.push(
        await listen<any>("config-updated", (ev) => {
          if (ev.payload?.ui) {
            applyUiSettings(normalizeUi(ev.payload.ui));
            contentLayout = ev.payload.ui.content_layout ?? contentLayout;
          }
        }),
      );

      try {
        debugLines = await invoke("get_debug_logs");
      } catch {
        /* ignore */
      }

      // Soft refresh of reminder list; firing/notify is handled by Rust poller
      const timer = setInterval(async () => {
        try {
          reminders = await invoke("list_reminders");
        } catch {
          /* ignore */
        }
      }, 30000);

      return () => {
        clearInterval(timer);
        unsubs.forEach((u) => u());
      };
    })();

    return () => unsubs.forEach((u) => u());
  });

  async function sendText(text: string) {
    const trimmed = text.trim();
    if (!trimmed || busy) return;
    busy = true;
    streaming = "";
    chatStatus = "";
    error = "";
    pendingQuestion = null;
    messages = [...messages, { role: "user", content: trimmed }];
    avatarState = "thinking";
    const t0 = Date.now();
    debugLines = [
      ...debugLines,
      {
        ts: String(Math.floor(t0 / 1000)),
        level: "info",
        message: `ui:send "${trimmed.slice(0, 80)}"`,
      },
    ];
    try {
      const res = await Promise.race([
        invoke<any>("chat", {
          request: { message: trimmed, sessionId },
        }),
        new Promise((_, reject) =>
          setTimeout(
            () =>
              reject(
                new Error(
                  "Timeout 150s: nessuna risposta da Ollama/agent. Apri tab Debug e premi Diagnostica.",
                ),
              ),
            150000,
          ),
        ),
      ]);
      sessionId = res.session_id;
      badge = res.badge;
      pending = res.pending_memory ?? pending;
      mcpPending = res.pending_mcp ?? null;
      pendingQuestion = res.pending_question ?? null;
      sessions = await invoke<typeof sessions>("list_sessions").catch(() => sessions);
      if ((res.pending_memory ?? []).length > 0) {
        memStatus = `${res.pending_memory.length} modifica/he memoria in attesa — approva in chat o in tab Memoria`;
      }
      try {
        memory = await invoke("get_memory");
      } catch {
        /* keep current snapshot */
      }
      void refreshOrganizer();
      const reply = (res.reply || streaming || "").trim();
      debugLines = [
        ...debugLines,
        {
          ts: String(Math.floor(Date.now() / 1000)),
          level: "info",
          message: `ui:reply_ms=${Date.now() - t0} chars=${reply.length}`,
        },
      ];
      if (!reply) {
        error = "Risposta vuota dal modello. Apri Debug → Diagnostica.";
      } else {
        messages = [...messages, { role: "assistant", content: reply }];
        // Sblocca subito la chat e avvia TTS non appena il testo è completo
        streaming = "";
        busy = false;
        // Se Rust ha già fatto TTS frase-per-frase (piper/audio8), non ripetere.
        if ((res as any)?.tts_streamed || (res as any)?.ttsStreamed) {
          avatarState = "speaking";
        } else {
          void (async () => {
            try {
              const fresh = await invoke<any>("get_config");
              if (fresh) config = fresh;
              const v = fresh?.voice ?? config?.voice;
              if (!v?.enabled || v?.auto_read_replies === false) {
                avatarState = "idle";
                return;
              }
              startReadingReply(reply, v, invoke, {
                onStart: () => {
                  avatarState = "speaking";
                },
                onDone: () => {
                  if (avatarState === "speaking") avatarState = "idle";
                },
                onError: (err) => {
                  error = `TTS: ${err}`;
                  debugLines = [
                    ...debugLines,
                    {
                      ts: String(Math.floor(Date.now() / 1000)),
                      level: "error",
                      message: `ui:tts_error ${err}`,
                    },
                  ].slice(-300);
                },
              });
            } catch (e) {
              error = `TTS: ${String(e)}`;
              avatarState = "idle";
            }
          })();
        }
      }
      streaming = "";
      ledger = await invoke("get_ledger");
      memory = await invoke("get_memory");
    } catch (e) {
      error = String(e);
      avatarState = "error";
      debugLines = [
        ...debugLines,
        {
          ts: String(Math.floor(Date.now() / 1000)),
          level: "error",
          message: `ui:chat_error ${String(e)}`,
        },
      ];
    } finally {
      busy = false;
      // Lo stato "speaking" lo gestisce onDone del TTS; qui non forzare idle.
    }
  }

  async function send() {
    const text = input.trim();
    if (!text || busy) return;
    input = "";
    await sendText(text);
  }

  async function approveMcpChat() {
    tab = "chat";
    if (busy) return;
    busy = true;
    streaming = "";
    chatStatus = "";
    error = "";
    avatarState = "thinking";
    try {
      const res = await invoke<any>("approve_and_resume_mcp");
      sessionId = res.session_id ?? sessionId;
      badge = res.badge ?? badge;
      pending = res.pending_memory ?? pending;
      mcpPending = res.pending_mcp ?? null;
      pendingQuestion = res.pending_question ?? null;
      const reply = (res.reply || streaming || "").trim();
      if (reply) {
        messages = [...messages, { role: "assistant", content: reply }];
        streaming = "";
      }
      sessions = await invoke<typeof sessions>("list_sessions").catch(() => sessions);
      avatarState = "idle";
    } catch (e) {
      try {
        await sendText("approva mcp");
      } catch {
        error = String(e);
        avatarState = "error";
      }
    } finally {
      busy = false;
    }
  }

  async function rejectMcpChat() {
    tab = "chat";
    try {
      await invoke("reject_pending_mcp");
    } catch {
      await sendText("rifiuta mcp");
    }
    mcpPending = null;
  }

  function pickOption(option: string) {
    tab = "chat";
    void sendText(option);
  }

  async function listenMic() {
    if (listening || busy) return;
    listening = true;
    error = "";
    try {
      const text = await invoke<string>("listen_transcribe", { seconds: 5 });
      if (text?.trim()) {
        input = (input ? input + " " : "") + text.trim();
      }
    } catch (e) {
      error = String(e);
    } finally {
      listening = false;
    }
  }

  async function speakBriefing() {
    if (!briefing?.speakable) return;
    avatarState = "speaking";
    await speakAssistantReply(briefing.speakable, { ...config?.voice, enabled: true, auto_read_replies: true }, invoke);
    avatarState = "idle";
  }

  async function probeAvatar() {
    probeBusy = true;
    copyStatus = "";
    try {
      await invoke("request_avatar_probe");
      copyStatus = "Sonda inviata all'avatar — i log `avatar:` arrivano qui sotto. Poi Copia dump.";
      await new Promise((r) => setTimeout(r, 600));
      debugLines = await invoke("get_debug_logs");
    } catch (e) {
      error = String(e);
    } finally {
      probeBusy = false;
    }
  }

  async function runDiag() {
    diagBusy = true;
    copyStatus = "";
    try {
      diagnostics = await invoke("run_diagnostics");
      debugLines = await invoke("get_debug_logs");
    } catch (e) {
      error = String(e);
    } finally {
      diagBusy = false;
    }
  }

  async function clearDebug() {
    await invoke("clear_debug_logs");
    debugLines = [];
    diagnostics = null;
    copyStatus = "";
  }

  async function copyDebug() {
    const text = [
      "=== AgentOS debug dump ===",
      `badge=${badge}`,
      `session=${sessionId}`,
      `error=${error}`,
      diagnostics ? `diagnostics=${JSON.stringify(diagnostics, null, 2)}` : "",
      "--- logs ---",
      ...debugLines.map((l) => `[${l.ts}] ${l.level}: ${l.message}`),
    ]
      .filter(Boolean)
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      copyStatus = "Copiato negli appunti";
    } catch {
      copyStatus = "Copia fallita — seleziona manualmente il log";
    }
  }

  async function startNewSession() {
    const s = await invoke<any>("new_session");
    sessionId = s.id;
    messages = [];
    streaming = "";
    pendingQuestion = null;
    mcpPending = null;
    sessions = await invoke<typeof sessions>("list_sessions").catch(() => sessions);
  }

  function formatSessionDate(iso: string): string {
    try {
      const d = new Date(iso);
      if (Number.isNaN(d.getTime())) return iso;
      return d.toLocaleString("it-IT", {
        day: "2-digit",
        month: "short",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return iso;
    }
  }

  function summarizeMcpArgs(args: unknown): string {
    try {
      const s = JSON.stringify(args ?? {}, null, 2);
      return s.length > 360 ? s.slice(0, 360) + "…" : s;
    } catch {
      return String(args ?? "");
    }
  }

  async function openSession(id: string) {
    if (id === sessionId) return;
    await invoke("set_active_session", { sessionId: id });
    sessionId = id;
    const rows = await invoke<any[]>("list_messages", { sessionId: id });
    messages = rows.map((r) => ({ role: r.role, content: r.content }));
    streaming = "";
    pendingQuestion = null;
  }

  async function approveMem(id: number) {
    await invoke("approve_memory", { id });
    pending = await invoke("list_pending_memory");
    memory = await invoke("get_memory");
    memStatus = "Memoria aggiornata e snapshot refreshato.";
  }

  async function rejectMem(id: number) {
    await invoke("reject_memory", { id });
    pending = await invoke("list_pending_memory");
    memStatus = "Richiesta rifiutata.";
  }

  async function removeMemory(id: number) {
    await invoke("delete_memory_entry", { id });
    memory = await invoke("get_memory");
    memStatus = "Entry eliminata.";
  }

  async function proposeMemory() {
    const content = memDraft.trim();
    if (!content) return;
    memStatus = "";
    try {
      const res = await invoke<any>("propose_memory", {
        store: memStore,
        content,
      });
      memDraft = "";
      pending = await invoke("list_pending_memory");
      memory = await invoke("get_memory");
      memStatus = res.applied
        ? "Salvato subito (write approval off)."
        : `In coda come #${res.pending_id} — approva sotto.`;
    } catch (e) {
      memStatus = String(e);
    }
  }

  async function runSearch() {
    searchHits = await invoke("session_search", { query: searchQ });
    if (!searchHits.length) memStatus = "Nessun hit FTS/LIKE.";
  }

  async function rebuildFts() {
    const n = await invoke<number>("rebuild_session_fts");
    memStatus = `FTS ricostruito: ${n} messaggi.`;
  }

  async function refreshOrganizer() {
    try {
      tasks = await invoke("list_tasks");
      reminders = await invoke("list_reminders");
      labels = await invoke("list_labels");
    } catch {
      /* ignore */
    }
  }

  function selectTab(id: Tab) {
    tab = id;
    if (id === "tasks") void refreshOrganizer();
    if (id === "mcp") void loadMcpStatus();
  }

  async function loadMcpStatus() {
    try {
      mcpStatus = await invoke("mcp_status");
      mcpServers = await invoke("list_mcp_servers");
      mcpPending = await invoke("get_pending_mcp");
    } catch (e) {
      mcpResult = String(e);
    }
  }

  async function saveSettings() {
    if (!config) return;
    settingsMsg = "";
    settingsSaving = true;
    try {
      if (!config.avatar) config.avatar = avatarSkin;
      else avatarSkin = normalizeAvatarSkin(config.avatar);
      await ensureVoiceDefaults();
      await invoke("save_app_config", { cfg: config });
      ensureUiDefaults();
      avatarSkin = normalizeAvatarSkin(config.avatar);
      voiceStatus = await invoke("voice_status").catch(() => voiceStatus);
      badge =
        config.provider === "ollama"
          ? "Locale · Ollama"
          : config.provider === "openai"
            ? "Cloud · OpenAI"
            : config.provider === "anthropic"
              ? "Cloud · Anthropic"
              : `Provider · ${config.provider}`;
      settingsMsg = "Configurazione salvata correttamente.";
    } catch (e) {
      settingsMsg = `Errore salvataggio: ${e}`;
    } finally {
      settingsSaving = false;
    }
  }

  function selectAvatar(id: string) {
    if (!config) return;
    config = { ...config, avatar: id };
    avatarSkin = id;
    settingsMsg = "";
  }

  async function testTts() {
    await ensureVoiceDefaults();
    avatarState = "speaking";
    await speakAssistantReply(
      "Ciao, sono AgentOS. La lettura vocale funziona.",
      { ...config.voice, enabled: true, auto_read_replies: true },
      invoke,
    );
    avatarState = "idle";
  }

  async function openSkill(id: string) {
    skillId = id;
    skillDraft = await invoke<string>("read_skill", { id });
  }

  async function saveSkill() {
    if (!skillId.trim()) return;
    await invoke("write_skill", { id: skillId.trim(), markdown: skillDraft });
    skillMsg = "Skill salvata.";
    await refreshSkills();
  }

  async function runSkillReview() {
    skillBusy = true;
    skillMsg = "";
    try {
      skillReview = await invoke("run_skills_review");
      skillMsg = "Review pronta — rivedi e applica se ti convince.";
    } catch (e) {
      skillMsg = String(e);
    } finally {
      skillBusy = false;
    }
  }

  async function applyReview() {
    if (!skillReview) return;
    await invoke("apply_skills_review", {
      id: skillReview.suggested_skill_id,
      markdown: skillReview.draft_markdown,
    });
    skillMsg = `Applicata skill «${skillReview.suggested_skill_id}».`;
    await refreshSkills();
  }

  async function saveKey() {
    if (!apiKeyDraft.trim()) return;
    await invoke("set_provider_key", {
      provider: config.provider,
      key: apiKeyDraft.trim(),
    });
    apiKeyDraft = "";
  }

  async function refreshMcpFromPanel(
    tools: any[],
    status: any,
    servers: any[],
  ) {
    mcpTools = tools;
    mcpStatus = status;
    mcpServers = servers;
  }

  async function refreshMcpTools() {
    await mcpPanelRef?.doReconnect?.();
  }

  async function approveMcp() {
    if (sessionId) {
      await approveMcpChat();
      return;
    }
    try {
      await invoke("approve_and_resume_mcp");
    } catch {
      await invoke("approve_pending_mcp");
    }
    mcpPending = null;
    ledger = await invoke("get_ledger");
  }

  async function rejectMcp() {
    if (sessionId) {
      await rejectMcpChat();
      return;
    }
    await invoke("reject_pending_mcp");
    mcpPending = null;
  }
</script>

<div class="shell">
  <aside class="rail">
    <div class="brand">
      {#key avatarSkin}
        <Avatar2D state={avatarState} size="sm" skin={avatarSkin} />
      {/key}
      <div>
        <h1>AgentOS</h1>
        <p class="tag">compagno locale</p>
      </div>
    </div>

    <div class="badge" title="Provider attivo">{badge}</div>

    <nav aria-label="Navigazione principale">
      {#each NAV_GROUPS as group}
        <div class="nav-group">
          <span class="nav-group-label">{group.label}</span>
          {#each group.items as item}
            <button
              class="nav-item"
              class:active={tab === item.id}
              title={item.hint}
              onclick={() => selectTab(item.id)}
            >
              <span class="nav-icon" aria-hidden="true">{item.icon}</span>
              <span class="nav-text">
                {item.label}{item.id === "memory" && pending.length ? ` (${pending.length})` : ""}
                <span class="nav-hint">{item.hint}</span>
              </span>
            </button>
          {/each}
        </div>
      {/each}
    </nav>

    <p class="hint">
      Suggerimenti chat: <code>salva un task: …</code>, <code>lista task</code>,
      <code>cosa ho fatto</code>, <code>ricordami tra 90 secondi di …</code>,
      <code>cerca online: …</code>
    </p>
  </aside>

  <main class="main-area">
    {#if error}
      <div class="banner">{error}</div>
    {/if}
    {#if tab === "chat" && pending.length}
      {#each pending as p}
        <div class="banner pending-mem">
          <span>
            Salvare in {p.store}: {p.content ?? p.old_text ?? p.action}
          </span>
          <span class="actions">
            <button type="button" onclick={() => approveMem(p.id)}>Approva</button>
            <button class="ghost" type="button" onclick={() => rejectMem(p.id)}>Rifiuta</button>
          </span>
        </div>
      {/each}
    {/if}
    {#if tab === "chat" && mcpPending}
      <div class="banner pending-mem mcp-approve-card">
        <div>
          <strong>Approvazione MCP</strong>
          <span class="muted"> · {mcpPending.level}</span>
          <p>
            <code>{mcpPending.server}/{mcpPending.tool}</code>
          </p>
          {#if mcpPending.arguments != null}
            <pre class="mcp-args">{summarizeMcpArgs(mcpPending.arguments)}</pre>
          {/if}
        </div>
        <span class="actions">
          <button type="button" disabled={busy} onclick={approveMcpChat}>Approva e continua</button>
          <button class="ghost" type="button" disabled={busy} onclick={rejectMcpChat}>Rifiuta</button>
        </span>
      </div>
    {/if}
    {#if tab === "chat" && pendingQuestion}
      <div class="banner pending-mem choice-banner">
        <div>
          <strong>{pendingQuestion.question}</strong>
          <div class="choice-chips">
            {#each pendingQuestion.options as opt}
              <button type="button" disabled={busy} onclick={() => pickOption(opt)}>{opt}</button>
            {/each}
          </div>
        </div>
      </div>
    {/if}

    {#if tab === "chat" && briefing && !briefingDismissed}
      <div class="banner soft briefing">
        <div>
          <strong>{briefing.greeting}</strong>
          <p>{briefing.narrative}</p>
        </div>
        <div class="actions">
          <button type="button" class="ghost" onclick={speakBriefing}>Ascolta</button>
          <button
            type="button"
            class="ghost"
            onclick={() => {
              briefingDismissed = true;
              briefing = null;
            }}>Chiudi</button
          >
        </div>
      </div>
    {/if}

    {#if tab === "chat"}
      <PageHeader title={TAB_META.chat.title} description={TAB_META.chat.description} breadcrumb={TAB_META.chat.breadcrumb}>
        {#snippet actions()}
          <button class="ghost" type="button" title="Ferma lettura" onclick={() => stopSpeaking(invoke)}>🔇</button>
          <button
            class="ghost"
            type="button"
            onclick={() => (showSessions = !showSessions)}
            title="Cronologia"
          >Cronologia</button>
          <button class="ghost" onclick={startNewSession}>Nuova sessione</button>
        {/snippet}
      </PageHeader>
      <div class="chat-layout" class:with-sessions={showSessions}>
        {#if showSessions}
          <aside class="sessions-rail" aria-label="Cronologia chat">
            <div class="sessions-head">
              <strong>Sessioni</strong>
              <button class="ghost" type="button" onclick={startNewSession}>＋</button>
            </div>
            <div class="sessions-list">
              {#each sessions as s}
                <button
                  type="button"
                  class="session-item"
                  class:active={s.id === sessionId}
                  onclick={() => openSession(s.id)}
                >
                  <span class="session-title">{s.title || "Chat"}</span>
                  <span class="session-date">{formatSessionDate(s.updated_at)}</span>
                </button>
              {:else}
                <p class="muted">Nessuna chat ancora.</p>
              {/each}
            </div>
          </aside>
        {/if}
        <aside class="companion" aria-label="Avatar AgentOS">
          {#key avatarSkin}
            <Avatar2D state={avatarState} size="lg" showLabel={true} name="AgentOS" skin={avatarSkin} />
          {/key}
          <p class="companion-note">Avatar 2D — la voce si abilita in Impostazioni → Voce.</p>
        </aside>
        <div class="chat-column">
          <section class="chat">
            {#if messages.length === 0 && !streaming && !busy}
              <div class="empty-chat">
                <p>Ciao — sono qui. Scrivi pure, oppure chiedimi di organizzare la giornata.</p>
              </div>
            {/if}
            {#each messages as m}
              <article class:user={m.role === "user"} class:assistant={m.role === "assistant"}>
                <span class="role">{m.role === "assistant" ? "AgentOS" : "tu"}</span>
                {#if m.role === "assistant"}
                  <MarkdownBody source={m.content} />
                {:else}
                  <p>{m.content}</p>
                {/if}
                {#if m.role === "assistant"}
                  <button
                    type="button"
                    class="ghost replay"
                    title="Rileggi"
                    onclick={() => {
                      startReadingReply(m.content, config?.voice, invoke, {
                        force: true,
                        onStart: () => (avatarState = "speaking"),
                        onDone: () => {
                          if (avatarState === "speaking") avatarState = "idle";
                        },
                        onError: (err) => (error = `TTS: ${err}`),
                      });
                    }}>🔊</button
                  >
                {/if}
              </article>
            {/each}
            {#if streaming}
              <article class="assistant streaming">
                <span class="role">AgentOS</span>
                <MarkdownBody source={streaming} />
              </article>
            {:else if busy}
              <article class="assistant streaming">
                <span class="role">AgentOS</span>
                <p>{chatStatus || "Sto elaborando… (la prima risposta può richiedere qualche secondo)"}</p>
              </article>
            {/if}
          </section>
          <form
            class="composer"
            onsubmit={(e) => {
              e.preventDefault();
              send();
            }}
          >
            <textarea
              rows="2"
              placeholder="Parla con AgentOS…"
              bind:value={input}
              onkeydown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  send();
                }
              }}
            ></textarea>
            <button
              type="button"
              class="ghost mic"
              disabled={listening || busy || !config?.voice?.stt_enabled}
              title={config?.voice?.stt_enabled ? "Ascolta 5s (Whisper)" : "Abilita STT in Settings"}
              onclick={listenMic}
            >
              {listening ? "…" : "🎙"}
            </button>
            <button type="submit" disabled={busy}>Invia</button>
          </form>
        </div>
      </div>
    {:else if tab === "memory"}
      <PageHeader title={TAB_META.memory.title} description={TAB_META.memory.description} breadcrumb={TAB_META.memory.breadcrumb} />
      {#if memStatus}
        <div class="banner">{memStatus}</div>
      {/if}
      <div class="content-grid" class:split={contentLayout === "split"} class:single={contentLayout === "single"}>
      {#if pending.length}
        <section class="panel warn full-width">
          <h3>In attesa di approvazione ({pending.length})</h3>
          <p class="panel-desc">Queste richieste di memoria aspettano la tua conferma prima di essere salvate.</p>
          {#each pending as p}
            <div class="row">
              <div>
                <strong>#{p.id} {p.action}</strong> → {p.store}
                <p>{p.content ?? p.old_text}</p>
              </div>
              <div class="actions">
                <button onclick={() => approveMem(p.id)}>Approva</button>
                <button class="ghost" onclick={() => rejectMem(p.id)}>Rifiuta</button>
              </div>
            </div>
          {/each}
        </section>
      {/if}
      <section class="panel">
        <h3>Aggiungi entry</h3>
        <p class="panel-desc">USER = profilo personale · MEMORY = note libere da ricordare.</p>
        <div class="inline">
          <select bind:value={memStore}>
            <option value="memory">MEMORY (note)</option>
            <option value="user">USER (profilo)</option>
          </select>
          <input placeholder="Testo da ricordare…" bind:value={memDraft} />
          <button onclick={proposeMemory}>Salva</button>
        </div>
      </section>
      {#if memory}
        <section class="panel">
          <h3>USER ({memory.user_chars}/{memory.user_limit})</h3>
          <p class="panel-desc">Preferenze e informazioni personali permanenti.</p>
          {#each memory.user as e}
            <div class="row">
              <p>{e.content}</p>
              <button class="ghost" onclick={() => removeMemory(e.id)}>Elimina</button>
            </div>
          {:else}
            <p class="muted">Nessuna entry. Prova: <code>preferisco risposte brevi</code></p>
          {/each}
        </section>
        <section class="panel">
          <h3>MEMORY ({memory.memory_chars}/{memory.memory_limit})</h3>
          <p class="panel-desc">Note e contesto aggiuntivo per le conversazioni.</p>
          {#each memory.memory as e}
            <div class="row">
              <p>{e.content}</p>
              <button class="ghost" onclick={() => removeMemory(e.id)}>Elimina</button>
            </div>
          {:else}
            <p class="muted">Nessuna entry. Prova: <code>ricorda il progetto si chiama AgentOS</code></p>
          {/each}
        </section>
      {/if}
      <section class="panel full-width">
        <h3>Ricerca nelle chat (FTS5)</h3>
        <p class="panel-desc">Cerca parole chiave nelle conversazioni passate.</p>
        <div class="inline">
          <input placeholder="parole da cercare nelle chat…" bind:value={searchQ} />
          <button onclick={runSearch}>Cerca</button>
          <button class="ghost" onclick={rebuildFts}>Rebuild FTS</button>
        </div>
        {#each searchHits as h}
          <p class="hit">[{h.role}] {h.content}</p>
        {:else}
          {#if searchQ.trim()}
            <p class="muted">Nessun risultato.</p>
          {/if}
        {/each}
      </section>
      </div>
    {:else if tab === "tasks"}
      <PageHeader title={TAB_META.tasks.title} description={TAB_META.tasks.description} breadcrumb={TAB_META.tasks.breadcrumb} />
      <OrganizerPanel {tasks} {reminders} {labels} onChanged={refreshOrganizer} />
    {:else if tab === "mcp"}
      <PageHeader title={TAB_META.mcp.title} description={TAB_META.mcp.description} breadcrumb={TAB_META.mcp.breadcrumb}>
        {#snippet actions()}
          <button class="ghost" onclick={refreshMcpTools}>Riconnetti</button>
        {/snippet}
      </PageHeader>
      <McpPanel
        bind:this={mcpPanelRef}
        {config}
        {mcpServers}
        {mcpTools}
        {mcpStatus}
        {mcpPending}
        {sessionId}
        onConfig={(cfg) => {
          config = cfg;
          mcpServers = cfg?.mcp_servers ?? mcpServers;
        }}
        onPending={(p) => {
          mcpPending = p;
        }}
        onLedger={async () => {
          ledger = await invoke("get_ledger");
        }}
        onApproveChat={approveMcpChat}
        onRejectChat={rejectMcpChat}
        onRefreshDone={refreshMcpFromPanel}
      />
    {:else if tab === "ledger"}
      <PageHeader title={TAB_META.ledger.title} description={TAB_META.ledger.description} breadcrumb={TAB_META.ledger.breadcrumb} />
      <section class="panel">
        {#each ledger as row}
          <div class="row">
            <div>
              <strong>{row.tool}</strong>
              <span class="muted"> · {row.permission} · {row.status}</span>
              <p>{row.summary}</p>
              <p class="muted">{row.timestamp}</p>
            </div>
          </div>
        {:else}
          <p class="muted">Nessuna azione ancora.</p>
        {/each}
      </section>
    {:else if tab === "settings"}
      <PageHeader title={TAB_META.settings.title} description={TAB_META.settings.description} breadcrumb={TAB_META.settings.breadcrumb} />
      {#if config}
        <div class="settings-layout">
          <nav class="settings-nav" aria-label="Sezioni impostazioni">
            {#each SETTINGS_SECTIONS as sec}
              <button
                type="button"
                class="settings-nav-item"
                class:active={settingsSection === sec.id}
                onclick={() => (settingsSection = sec.id)}
              >
                <span class="settings-nav-icon" aria-hidden="true">{sec.icon}</span>
                <span class="settings-nav-text">
                  <strong>{sec.label}</strong>
                  <span class="settings-nav-blurb">{sec.blurb}</span>
                </span>
              </button>
            {/each}
          </nav>

          <div class="settings-content">
            {#if settingsSection === "appearance"}
              <section class="panel stack">
                <h3>Aspetto e layout</h3>
                <p class="panel-desc">Personalizza colori e disposizione dell'interfaccia. Le modifiche si vedono subito; clicca Salva per mantenerle.</p>

                <h4>Tema</h4>
                <div class="theme-picker">
                  {#each THEME_PRESETS as preset}
                    <button
                      type="button"
                      class="theme-card"
                      class:active={config.ui?.theme === preset.id}
                      onclick={() => {
                        config.ui.theme = preset.id;
                        previewUi();
                      }}
                    >
                      <span class="theme-swatch" style="background: {preset.swatch}"></span>
                      <strong>{preset.name}</strong>
                      <span class="muted">{preset.blurb}</span>
                    </button>
                  {/each}
                </div>

                <h4>Colore accento</h4>
                <div class="accent-picker">
                  {#each ACCENT_PRESETS as accent}
                    <button
                      type="button"
                      class="accent-chip"
                      class:active={config.ui?.accent === accent.id}
                      onclick={() => {
                        config.ui.accent = accent.id;
                        previewUi();
                      }}
                    >
                      <span class="accent-dot" style="background: {accent.swatch}"></span>
                      {accent.name}
                    </button>
                  {/each}
                </div>

                <label>
                  Densità interfaccia
                  <select
                    bind:value={config.ui.density}
                    onchange={previewUi}
                  >
                    <option value="comfortable">Comoda — più spazio tra gli elementi</option>
                    <option value="compact">Compatta — più contenuti visibili</option>
                  </select>
                </label>

                <label>
                  Layout sezioni (Memoria, Task, MCP…)
                  <select
                    bind:value={config.ui.content_layout}
                    onchange={previewUi}
                  >
                    <option value="split">Due colonne — pannelli affiancati</option>
                    <option value="single">Una colonna — pannelli impilati</option>
                  </select>
                </label>

                <div class="settings-actions">
                  <button type="button" disabled={settingsSaving} onclick={saveSettings}>
                    {settingsSaving ? "Salvataggio…" : "Salva aspetto"}
                  </button>
                  {#if settingsMsg}
                    <p class:settings-ok={!settingsMsg.startsWith("Errore")} class:settings-err={settingsMsg.startsWith("Errore")}>
                      {settingsMsg}
                    </p>
                  {/if}
                </div>
              </section>

            {:else if settingsSection === "avatar"}
              <section class="panel stack">
                <h3>Avatar del compagno</h3>
                <p class="panel-desc">Scegli l'aspetto, poi salva per applicarlo ovunque (console e avatar flottante).</p>
                <div class="avatar-picker">
                  {#each AVATAR_SKINS as skin}
                    <button
                      type="button"
                      class="skin-card"
                      class:active={(config.avatar ?? avatarSkin) === skin.id}
                      onclick={() => selectAvatar(skin.id)}
                    >
                      <div class="skin-preview">
                        <Avatar2D state="idle" size="sm" skin={skin.id} hideCrest={skin.id === "familiar"} />
                      </div>
                      <strong>{skin.name}</strong>
                      <span class="muted">{skin.blurb}</span>
                    </button>
                  {/each}
                </div>
                <label>
                  Cammina sul desktop
                  <input type="checkbox" bind:checked={config.avatar_roam} />
                </label>
                <p class="muted">Se attivo, l'avatar si sposta sul monitor. Si ferma se trascini o apri la chat.</p>
                <div class="avatar-preview-states">
                  <span class="muted">Anteprima stati:</span>
                  {#key avatarSkin}
                    <Avatar2D state="idle" size="sm" skin={avatarSkin} />
                    <Avatar2D state="thinking" size="sm" skin={avatarSkin} />
                    <Avatar2D state="speaking" size="sm" skin={avatarSkin} />
                  {/key}
                </div>
                <div class="settings-actions">
                  <button type="button" disabled={settingsSaving} onclick={saveSettings}>
                    {settingsSaving ? "Salvataggio…" : "Salva avatar"}
                  </button>
                  {#if settingsMsg}
                    <p class:settings-ok={!settingsMsg.startsWith("Errore")} class:settings-err={settingsMsg.startsWith("Errore")}>
                      {settingsMsg}
                    </p>
                  {/if}
                </div>
              </section>

            {:else if settingsSection === "provider"}
              <section class="panel stack">
                <h3>Provider AI</h3>
                <p class="panel-desc">Configura quale modello risponde alle chat e come si connette.</p>
                <label>
                  Provider
                  <select bind:value={config.provider}>
                    <option value="ollama">Ollama (locale)</option>
                    <option value="openai">OpenAI (cloud)</option>
                    <option value="anthropic">Anthropic (cloud)</option>
                  </select>
                </label>
                <label>
                  Modello
                  <input bind:value={config.model} list="models" />
                  <datalist id="models">
                    {#each models as m}
                      <option value={m}></option>
                    {/each}
                  </datalist>
                </label>
                <label>
                  Ollama URL
                  <input bind:value={config.ollama.base_url} />
                </label>
                <label>
                  Approvazione scritture memoria
                  <input type="checkbox" bind:checked={config.memory.write_approval} />
                </label>
                <p class="muted">Se attivo, AgentOS chiede conferma prima di salvare nuove informazioni su di te.</p>
                <label>
                  Permesso MCP predefinito
                  <select bind:value={config.permissions.default_mcp}>
                    <option value="safe">safe — solo azioni sicure</option>
                    <option value="read">read — lettura dati</option>
                    <option value="operate">operate — operazioni standard</option>
                    <option value="admin">admin — accesso completo</option>
                  </select>
                </label>
                <p class="muted">
                  MCP collega AgentOS a strumenti esterni (file, git, browser, API, finance, …). Questo livello è il cancello:
                  <strong>safe</strong> e <strong>read</strong> eseguono subito;
                  <strong>operate</strong> (consigliato) e <strong>admin</strong> chiedono Approva/Rifiuta nel tab MCP prima di agire.
                  Con server collegati, in chat puoi parlare in linguaggio naturale: AgentOS sceglie i tool da solo.
                  Restano disponibili <code>mcp list</code>, <code>mcp call …</code> e <code>approva mcp</code>.
                </p>
                {#if config.provider !== "ollama"}
                  <label>
                    API key ({config.provider}) — salvata nel keyring del sistema
                    <input type="password" bind:value={apiKeyDraft} placeholder="sk-…" />
                  </label>
                  <button onclick={saveKey}>Salva chiave</button>
                {/if}
                <div class="settings-actions">
                  <button type="button" disabled={settingsSaving} onclick={saveSettings}>
                    {settingsSaving ? "Salvataggio…" : "Salva provider"}
                  </button>
                  {#if settingsMsg}
                    <p class:settings-ok={!settingsMsg.startsWith("Errore")} class:settings-err={settingsMsg.startsWith("Errore")}>
                      {settingsMsg}
                    </p>
                  {/if}
                </div>
              </section>

            {:else if settingsSection === "voice"}
              <section class="panel stack">
                <h3>Voce</h3>
                <p class="panel-desc">
                  Lettura risposte (TTS) e input microfono (Whisper). Setup:
                  <code>scripts/setup-voice.sh</code> · Audio8:
                  <code>scripts/setup-audio8.sh</code>
                </p>
                <label>
                  Abilita TTS
                  <input type="checkbox" bind:checked={config.voice.enabled} />
                </label>
                <label>
                  Leggi automaticamente le risposte
                  <input type="checkbox" bind:checked={config.voice.auto_read_replies} />
                </label>
                <label>
                  Motore TTS
                  <select bind:value={config.voice.engine}>
                    <option value="system">System (Web Speech)</option>
                    <option value="piper">Piper (locale)</option>
                    <option value="audio8">Audio8 0.6B</option>
                  </select>
                </label>
                <label>
                  Voce / modello Piper o system
                  <input bind:value={config.voice.voice} placeholder="it_IT-riccardo-x_low oppure nome voce OS" />
                </label>
                <label>
                  Velocità ({config.voice.rate})
                  <input type="range" min="0.5" max="2" step="0.05" bind:value={config.voice.rate} />
                </label>
                <label>
                  Volume ({config.voice.volume})
                  <input type="range" min="0.1" max="1" step="0.05" bind:value={config.voice.volume} />
                </label>
                <label>
                  Piper binary (opzionale)
                  <input bind:value={config.voice.piper_bin} placeholder="auto" />
                </label>
                <label>
                  Piper data dir (opzionale)
                  <input bind:value={config.voice.piper_data_dir} placeholder="~/.agentos/voice/piper" />
                </label>

                <h4>Audio8</h4>
                <p class="muted">Con Ollama sulla GPU usa <strong>cpu</strong> (ONNX). <strong>cuda</strong> compete sulla VRAM.</p>
                <label>
                  Device Audio8
                  <select bind:value={config.voice.audio8_device}>
                    <option value="cpu">CPU (ONNX — consigliato)</option>
                    <option value="cuda">GPU / CUDA</option>
                  </select>
                </label>
                <label>
                  Audio8 base URL
                  <input bind:value={config.voice.audio8_base_url} placeholder="http://127.0.0.1:8024" />
                </label>
                <label>
                  Voce Audio8
                  <input bind:value={config.voice.audio8_voice} placeholder="speaker_a" />
                </label>
                <label>
                  Lettura frase-per-frase durante lo streaming
                  <input type="checkbox" bind:checked={config.voice.stream_sentences} />
                </label>
                <label>
                  Model id (solo CUDA)
                  <input bind:value={config.voice.audio8_model_id} placeholder="Audio8/Audio8-TTS-Preview-0.6b" />
                </label>
                <label>
                  Runtime dir ONNX (opzionale)
                  <input bind:value={config.voice.audio8_runtime_dir} placeholder="~/.agentos/voice/audio8/..." />
                </label>
                <label>
                  Abilita input vocale (Whisper)
                  <input type="checkbox" bind:checked={config.voice.stt_enabled} />
                </label>
                <label>
                  Whisper binary
                  <input bind:value={config.voice.whisper_bin} placeholder="whisper-cli" />
                </label>
                <label>
                  Whisper model path
                  <input bind:value={config.voice.whisper_model} placeholder="~/.agentos/voice/whisper/ggml-base.bin" />
                </label>
                <div class="actions">
                  <button type="button" onclick={saveSettings}>Salva voce</button>
                  <button type="button" class="ghost" onclick={testTts}>Prova TTS</button>
                </div>
                {#if voiceStatus}
                  <p class="muted">
                    Piper: {voiceStatus.piper_available ? "ok" : "no"} · Whisper:
                    {voiceStatus.whisper_available ? "ok" : "no"} · Audio8:
                    {voiceStatus.audio8_available ? "ok" : "no"}
                    ({voiceStatus.audio8_device ?? "cpu"})
                  </p>
                  {#each voiceStatus.notes ?? [] as note}
                    <p class="muted">{note}</p>
                  {/each}
                {/if}
              </section>

            {:else if settingsSection === "briefing"}
              <section class="panel stack">
                <h3>Briefing mattutino</h3>
                <p class="panel-desc">Un riassunto giornaliero all'apertura della chat, con task e promemoria in scadenza.</p>
                <label>
                  Abilita briefing
                  <input type="checkbox" bind:checked={config.briefing.enabled} />
                </label>
                <label>
                  Ora locale (0 = sempre disponibile)
                  <input type="number" min="0" max="23" bind:value={config.briefing.hour} />
                </label>
                <label>
                  Lucida con LLM
                  <input type="checkbox" bind:checked={config.briefing.llm_polish} />
                </label>
                <p class="muted">Se attivo, il testo del briefing viene riscritto dal modello per un tono più naturale.</p>
                <button type="button" onclick={saveSettings}>Salva briefing</button>
              </section>

            {:else if settingsSection === "skills"}
              <section class="panel stack">
                <h3>Skills nel prompt</h3>
                <p class="panel-desc">Controlla come le competenze procedurali vengono incluse nelle conversazioni.</p>
                <label>
                  Abilita skills nel prompt
                  <input type="checkbox" bind:checked={config.skills.enabled} />
                </label>
                <label>
                  Inietta il contenuto completo delle skills
                  <input type="checkbox" bind:checked={config.skills.inject_prompt} />
                </label>
                <button type="button" onclick={saveSettings}>Salva skills</button>
              </section>
            {/if}
          </div>
        </div>
      {/if}
    {:else if tab === "skills"}
      <PageHeader title={TAB_META.skills.title} description={TAB_META.skills.description} breadcrumb={TAB_META.skills.breadcrumb}>
        {#snippet actions()}
          <button type="button" onclick={runSkillReview} disabled={skillBusy}>
            {skillBusy ? "Review…" : "Background review"}
          </button>
        {/snippet}
      </PageHeader>
      {#if skillMsg}
        <div class="banner soft">{skillMsg}</div>
      {/if}
      <div class="content-grid" class:split={contentLayout === "split"} class:single={contentLayout === "single"}>
      <section class="panel">
        <h3>Installate</h3>
        <p class="panel-desc">Skills attualmente disponibili per AgentOS.</p>
        {#each skills as s}
          <div class="row">
            <div>
              <strong>{s.name}</strong>
              <span class="muted"> · {s.id}{s.enabled ? "" : " · off"}</span>
              <p>{s.description}</p>
            </div>
            <button class="ghost" type="button" onclick={() => openSkill(s.id)}>Apri</button>
          </div>
        {:else}
          <p class="muted">Nessuna skill (verrà creata la seed companion-tone).</p>
        {/each}
      </section>
      {#if skillReview}
        <section class="panel stack">
          <h3>Ultima review</h3>
          <p class="panel-desc">Proposta generata dall'ultima revisione automatica.</p>
          <p>{skillReview.summary}</p>
          <p class="muted">Proposta: {skillReview.suggested_skill_id}</p>
          <pre class="skill-pre">{skillReview.draft_markdown}</pre>
          <button type="button" onclick={applyReview}>Applica proposta</button>
        </section>
      {/if}
      <section class="panel stack full-width">
        <h3>Editor</h3>
        <p class="panel-desc">Crea o modifica una skill in formato Markdown con frontmatter YAML.</p>
        <label>
          Skill id
          <input bind:value={skillId} placeholder="mia-skill" />
        </label>
        <textarea rows="14" bind:value={skillDraft} placeholder="---&#10;name: …&#10;description: …&#10;enabled: true&#10;---&#10;"
        ></textarea>
        <button type="button" onclick={saveSkill}>Salva skill</button>
      </section>
      </div>
    {:else}
      <PageHeader title={TAB_META.debug.title} description={TAB_META.debug.description} breadcrumb={TAB_META.debug.breadcrumb}>
        {#snippet actions()}
          <button onclick={runDiag} disabled={diagBusy}>
            {diagBusy ? "Diagnostica…" : "Diagnostica"}
          </button>
          <button onclick={probeAvatar} disabled={probeBusy}>
            {probeBusy ? "Sonda…" : "Sonda avatar"}
          </button>
          <button class="ghost" onclick={copyDebug}>Copia dump</button>
          <button class="ghost" onclick={clearDebug}>Pulisci</button>
        {/snippet}
      </PageHeader>
      {#if copyStatus}
        <p class="muted">{copyStatus}</p>
      {/if}
      <div class="content-grid single">
      <section class="panel stack">
        <h3>Compositor avatar (cloni)</h3>
        <p class="panel-desc">
          Su Linux (WebKitGTK) i pixel trasparenti possono creare scie. Su Windows e macOS
          la finestra usa vetro con alpha per-pixel.
        </p>
      </section>
      {#if diagnostics}
        <section class="panel">
          <h3>Risultato diagnostica</h3>
          <pre class="debug-pre">{JSON.stringify(diagnostics, null, 2)}</pre>
        </section>
      {/if}
      <section class="panel">
        <h3>Log live (~/.agentos/logs/agentos.log)</h3>
        <pre class="debug-pre">{#each debugLines as l}[{l.ts}] {l.level}: {l.message}
{/each}</pre>
      </section>
      </div>
    {/if}
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 270px 1fr;
    min-height: 100vh;
    background-color: var(--main-bg, var(--bg0));
    background-image:
      radial-gradient(1200px 600px at 10% -10%, var(--shell-gradient-a), transparent 55%),
      radial-gradient(900px 500px at 100% 0%, var(--shell-gradient-b), transparent 50%),
      linear-gradient(165deg, var(--bg0), color-mix(in srgb, var(--bg0) 85%, var(--bg1)) 45%, var(--bg0));
  }

  .rail {
    padding: var(--space-lg);
    border-right: 1px solid var(--line);
    background: var(--rail-bg);
    backdrop-filter: blur(12px);
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
  }

  .brand {
    display: flex;
    gap: var(--space-md);
    align-items: center;
  }

  .brand h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 1.45rem;
    letter-spacing: 0.02em;
  }

  .tag {
    margin: 0.1rem 0 0;
    color: var(--muted);
    font-size: 0.85rem;
  }

  .badge {
    display: inline-flex;
    align-self: flex-start;
    padding: 0.35rem 0.65rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--accent);
    font-size: 0.8rem;
    background: var(--accent-soft);
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    overflow-y: auto;
  }

  .nav-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
  }

  .nav-group-label {
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
    padding: 0 0.35rem;
    margin-bottom: 0.1rem;
  }

  .nav-item {
    display: flex;
    align-items: flex-start;
    gap: 0.55rem;
    text-align: left;
    border: 1px solid transparent;
    background: transparent;
    color: var(--ink);
    border-radius: var(--radius-md);
    padding: 0.55rem 0.65rem;
    transition: background 0.15s, border-color 0.15s;
  }

  .nav-item:hover {
    background: color-mix(in srgb, var(--bg2) 60%, transparent);
    border-color: var(--line);
  }

  .nav-item.active {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    background: var(--accent-soft);
  }

  .nav-icon {
    font-size: 1rem;
    line-height: 1.2;
    flex-shrink: 0;
  }

  .nav-text {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    font-size: 0.88rem;
    font-weight: 500;
  }

  .nav-hint {
    font-size: 0.72rem;
    font-weight: 400;
    color: var(--muted);
    line-height: 1.3;
  }

  .nav-item.active .nav-hint {
    color: color-mix(in srgb, var(--muted) 80%, var(--accent));
  }

  .ghost,
  button {
    border: 1px solid var(--line);
    background: var(--bg2);
    color: var(--ink);
    border-radius: var(--radius-md);
    padding: var(--space-sm) var(--space-md);
    transition: border-color 0.15s;
  }

  .ghost:hover:not(:disabled),
  button:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent) 35%, var(--line));
  }

  .ghost {
    background: transparent;
  }

  .hint {
    margin-top: auto;
    color: var(--muted);
    font-size: 0.78rem;
    line-height: 1.4;
  }

  .main-area {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    padding: var(--space-lg);
    overflow: hidden;
    background: transparent;
    color: var(--ink);
  }

  .banner {
    margin-bottom: var(--space-md);
    padding: var(--space-sm) var(--space-md);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--danger) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
  }

  .pending-mem {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-md);
    background: var(--warn-bg);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .choice-banner {
    align-items: flex-start;
  }

  .choice-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-top: 0.55rem;
  }

  .choice-chips button {
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--line));
    background: color-mix(in srgb, var(--accent-soft) 50%, var(--bg2));
    color: var(--ink);
    border-radius: 999px;
    padding: 0.4rem 0.85rem;
    cursor: pointer;
    font-size: 0.82rem;
  }

  .choice-chips button:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .banner.soft,
  .banner.briefing {
    background: var(--success-bg);
    border-color: color-mix(in srgb, var(--accent-2) 35%, transparent);
  }

  .briefing {
    display: flex;
    justify-content: space-between;
    gap: var(--space-lg);
    align-items: flex-start;
  }

  .briefing p {
    margin: 0.35rem 0 0;
    white-space: pre-wrap;
  }

  .content-grid {
    display: grid;
    gap: var(--space-md);
    flex: 1;
    overflow-y: auto;
    align-content: start;
    padding-bottom: var(--space-md);
  }

  .content-grid.split {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .content-grid.single {
    grid-template-columns: 1fr;
  }

  .content-grid .full-width {
    grid-column: 1 / -1;
  }

  @media (max-width: 960px) {
    .shell {
      grid-template-columns: 1fr;
    }
    .rail {
      border-right: 0;
      border-bottom: 1px solid var(--line);
    }
    .content-grid.split {
      grid-template-columns: 1fr;
    }
    .settings-layout {
      grid-template-columns: 1fr !important;
    }
  }

  .chat-layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 210px 1fr;
    gap: var(--space-lg);
  }

  .chat-layout.with-sessions {
    grid-template-columns: 200px 210px 1fr;
  }

  .sessions-rail {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-radius: var(--radius-lg);
    border: 1px solid var(--line);
    background: var(--panel-bg);
    overflow: hidden;
  }

  .sessions-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.55rem 0.7rem;
    border-bottom: 1px solid var(--line);
    font-size: 0.85rem;
  }

  .sessions-list {
    overflow: auto;
    padding: 0.4rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .session-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.1rem;
    text-align: left;
    border: 1px solid transparent;
    background: transparent;
    color: var(--ink);
    border-radius: 8px;
    padding: 0.45rem 0.55rem;
    cursor: pointer;
  }

  .session-item:hover,
  .session-item.active {
    border-color: var(--line);
    background: color-mix(in srgb, var(--accent-soft) 40%, var(--bg2));
  }

  .session-title {
    font-size: 0.82rem;
    font-weight: 600;
  }

  .session-date {
    font-size: 0.7rem;
    color: var(--muted);
  }

  .mcp-approve-card {
    align-items: flex-start;
  }

  .mcp-args {
    margin: 0.35rem 0 0;
    padding: 0.45rem 0.55rem;
    max-height: 7rem;
    overflow: auto;
    font-size: 0.75rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: color-mix(in srgb, var(--bg0) 80%, transparent);
    white-space: pre-wrap;
  }

  .companion {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-start;
    gap: var(--space-md);
    padding: var(--space-lg) var(--space-md);
    border-radius: var(--radius-lg);
    border: 1px solid var(--line);
    background:
      radial-gradient(140px 120px at 50% 28%, var(--accent-soft), transparent 70%),
      var(--panel-bg);
  }

  .companion-note {
    margin: 0;
    text-align: center;
    color: var(--muted);
    font-size: 0.72rem;
    line-height: 1.35;
    max-width: 11.5rem;
  }

  .chat-column {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .chat {
    flex: 1;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    padding: var(--space-sm);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel-bg);
  }

  .empty-chat {
    margin: auto 0;
    padding: var(--space-lg) var(--space-md);
    text-align: center;
    color: var(--muted);
    font-size: 0.95rem;
    line-height: 1.45;
  }

  @media (max-width: 820px) {
    .chat-layout {
      grid-template-columns: 1fr;
    }
    .chat-layout.with-sessions {
      grid-template-columns: 1fr;
    }
    .sessions-rail {
      max-height: 10rem;
    }
    .companion {
      flex-direction: row;
      justify-content: flex-start;
      gap: var(--space-lg);
      padding: var(--space-sm) var(--space-md);
    }
    .companion-note {
      text-align: left;
      max-width: none;
    }
  }

  article {
    max-width: 78%;
    padding: var(--space-md) 0.95rem;
    border-radius: var(--radius-lg);
    border: 1px solid var(--line);
    background: color-mix(in srgb, var(--bg2) 75%, transparent);
  }

  article.user {
    align-self: flex-end;
    background: var(--user-bubble);
    border-color: color-mix(in srgb, var(--accent) 25%, var(--line));
  }

  article.assistant {
    align-self: flex-start;
    position: relative;
  }

  .replay {
    position: absolute;
    top: 0.35rem;
    right: 0.35rem;
    min-width: 1.8rem !important;
    padding: 0.15rem 0.35rem !important;
    opacity: 0.55;
    font-size: 0.85rem;
  }

  .replay:hover {
    opacity: 1;
  }

  .role {
    display: block;
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
    margin-bottom: 0.25rem;
  }

  article > p {
    margin: 0;
    white-space: pre-wrap;
    line-height: 1.45;
  }

  .composer {
    margin-top: var(--space-md);
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: var(--space-sm);
  }

  .mic {
    min-width: 2.5rem;
  }

  .panel {
    border: 1px solid var(--line);
    background: var(--panel-bg);
    border-radius: var(--radius-lg);
    padding: var(--space-md) var(--space-lg);
  }

  .panel.warn {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    background: var(--warn-bg);
  }

  .panel h3 {
    margin: 0 0 var(--space-sm);
    font-size: 0.95rem;
    font-weight: 600;
  }

  .panel h4 {
    margin: var(--space-md) 0 var(--space-xs);
    font-size: 0.88rem;
    color: var(--muted);
  }

  .panel-desc {
    margin: 0 0 var(--space-md);
    color: var(--muted);
    font-size: 0.85rem;
    line-height: 1.45;
  }

  .row {
    display: flex;
    justify-content: space-between;
    gap: var(--space-md);
    align-items: flex-start;
    padding: var(--space-sm) 0;
    border-top: 1px solid var(--line);
  }

  .row:first-of-type {
    border-top: 0;
  }

  .actions {
    display: flex;
    gap: var(--space-xs);
    flex-shrink: 0;
  }

  .inline {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    margin-bottom: var(--space-sm);
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }

  /* Settings layout */
  .settings-layout {
    display: grid;
    grid-template-columns: 220px 1fr;
    gap: var(--space-lg);
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .settings-nav {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
    overflow-y: auto;
  }

  .settings-nav-item {
    display: flex;
    align-items: flex-start;
    gap: 0.55rem;
    text-align: left;
    border: 1px solid transparent;
    background: transparent;
    color: var(--ink);
    border-radius: var(--radius-md);
    padding: 0.6rem 0.7rem;
    transition: background 0.15s, border-color 0.15s;
  }

  .settings-nav-item:hover {
    background: color-mix(in srgb, var(--bg2) 50%, transparent);
    border-color: var(--line);
  }

  .settings-nav-item.active {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    background: var(--accent-soft);
  }

  .settings-nav-icon {
    font-size: 1rem;
    flex-shrink: 0;
  }

  .settings-nav-text {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .settings-nav-text strong {
    font-size: 0.86rem;
    font-weight: 600;
  }

  .settings-nav-blurb {
    font-size: 0.72rem;
    color: var(--muted);
    line-height: 1.3;
  }

  .settings-content {
    overflow-y: auto;
    min-height: 0;
  }

  /* Theme picker */
  .theme-picker {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: var(--space-sm);
    margin-bottom: var(--space-md);
  }

  .theme-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.3rem;
    text-align: left;
    padding: 0.75rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: transparent;
    color: inherit;
    cursor: pointer;
    transition: border-color 0.15s;
  }

  .theme-card.active {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .theme-swatch {
    width: 100%;
    height: 36px;
    border-radius: 6px;
    border: 1px solid var(--line);
    margin-bottom: 0.2rem;
  }

  .accent-picker {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    margin-bottom: var(--space-md);
  }

  .accent-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.45rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font-size: 0.85rem;
  }

  .accent-chip.active {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .accent-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .avatar-picker {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--space-md);
  }

  .skin-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.35rem;
    text-align: left;
    padding: var(--space-md);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }

  .skin-card.active {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .skin-preview {
    width: 100%;
    display: grid;
    place-items: center;
    min-height: 72px;
    margin-bottom: 0.25rem;
  }

  .avatar-preview-states {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    flex-wrap: wrap;
    margin-top: 0.35rem;
  }

  .settings-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-md);
    margin-top: var(--space-sm);
    padding-top: var(--space-md);
    border-top: 1px solid var(--line);
  }

  .settings-ok {
    margin: 0;
    color: var(--accent-2);
    font-size: 0.9rem;
  }

  .settings-err {
    margin: 0;
    color: var(--danger);
    font-size: 0.9rem;
  }

  .muted {
    color: var(--muted);
  }

  .hit {
    font-size: 0.9rem;
    border-top: 1px solid var(--line);
    padding-top: var(--space-xs);
  }

  pre.hit {
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 240px;
    overflow: auto;
    font-family: ui-monospace, monospace;
    font-size: 0.82rem;
  }

  .skill-pre {
    max-height: 240px;
    overflow: auto;
    padding: var(--space-md);
    border-radius: var(--radius-md);
    border: 1px solid var(--line);
    background: color-mix(in srgb, var(--bg0) 80%, transparent);
    font-size: 0.8rem;
    white-space: pre-wrap;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
    font-size: 0.9rem;
  }

  .debug-pre {
    margin: 0;
    max-height: 50vh;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: ui-monospace, "Cascadia Code", monospace;
    font-size: 0.78rem;
    line-height: 1.4;
    color: var(--muted);
  }
</style>
