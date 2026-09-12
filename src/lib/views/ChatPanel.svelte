<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { emit, listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Avatar2D from "$lib/components/Avatar2D.svelte";
  import MarkdownBody from "$lib/components/MarkdownBody.svelte";
  import type { AvatarState } from "$lib/avatar";
  import { normalizeAvatarSkin } from "$lib/avatar";
  import { startReadingReply, stopSpeaking } from "$lib/voice";
  import { applyUiSettings, normalizeUi } from "$lib/theme";

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
  let pendingItems = $state<
    { id: number; store: string; action: string; content?: string; old_text?: string }[]
  >([]);
  let mcpPending = $state<{
    server: string;
    tool: string;
    arguments?: unknown;
    level: string;
  } | null>(null);
  let pendingQuestion = $state<{
    id: string;
    question: string;
    options: string[];
    allowFreeText?: boolean;
    allow_free_text?: boolean;
  } | null>(null);
  let voiceCfg = $state<any>(null);
  let listening = $state(false);
  let alertMsg = $state("");
  let sessions = $state<{ id: string; title: string; updated_at: string }[]>([]);
  let showSessions = $state(false);

  function summarizeArgs(args: unknown): string {
    try {
      const s = JSON.stringify(args ?? {}, null, 2);
      return s.length > 360 ? s.slice(0, 360) + "…" : s;
    } catch {
      return String(args ?? "");
    }
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

  async function refreshSessions() {
    try {
      sessions = await invoke<typeof sessions>("list_sessions");
    } catch {
      /* ignore */
    }
  }

  onMount(() => {
    let unsubs: (() => void)[] = [];
    (async () => {
      try {
        const data = await invoke<any>("get_bootstrap");
        badge = data.badge;
        avatarSkin = normalizeAvatarSkin(data.config?.avatar);
        voiceCfg = data.config?.voice ?? null;
        if (data.config?.ui) applyUiSettings(normalizeUi(data.config.ui));
        sessionId = data.active_session;
        pendingItems = data.pending_memory ?? [];
        mcpPending = data.pending_mcp ?? null;
        pendingQuestion = data.pending_question ?? null;
        sessions = data.sessions ?? [];
        if (sessionId) {
          const rows = await invoke<any[]>("list_messages", { sessionId });
          messages = rows.map((r) => ({ role: r.role, content: r.content }));
        }
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
          if (alertMsg && ev.payload !== "notification") return;
          avatarState = ev.payload as AvatarState;
        }),
      );
      unsubs.push(
        await listen<any>("reminder-due", (ev) => {
          alertMsg = String(ev.payload?.message ?? "Hai un promemoria");
          avatarState = "notification";
        }),
      );
      unsubs.push(
        await listen("reminder-acked", () => {
          alertMsg = "";
          if (avatarState === "notification") avatarState = "idle";
        }),
      );
      unsubs.push(
        await listen("memory-changed", async () => {
          pendingItems = await invoke<typeof pendingItems>("list_pending_memory").catch(
            () => pendingItems,
          );
        }),
      );
      unsubs.push(
        await listen<any>("mcp-pending", (ev) => {
          mcpPending = ev.payload ?? null;
        }),
      );
      unsubs.push(
        await listen<any>("config-updated", (ev) => {
          if (ev.payload?.avatar != null) {
            avatarSkin = normalizeAvatarSkin(ev.payload.avatar);
          }
          if (ev.payload?.ui) {
            applyUiSettings(normalizeUi(ev.payload.ui));
          }
        }),
      );
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
    try {
      const res = await Promise.race([
        invoke<any>("chat", {
          request: { message: trimmed, sessionId },
        }),
        new Promise((_, reject) =>
          setTimeout(
            () => reject(new Error("Timeout 150s: nessuna risposta. Apri la console dal tray.")),
            150000,
          ),
        ),
      ]);
      sessionId = res.session_id;
      badge = res.badge;
      pendingItems = res.pending_memory ?? pendingItems;
      mcpPending = res.pending_mcp ?? null;
      pendingQuestion = res.pending_question ?? null;
      await refreshSessions();
      const reply = (res.reply || streaming || "").trim();
      if (!reply) {
        error = "Risposta vuota dal modello.";
      } else {
        messages = [...messages, { role: "assistant", content: reply }];
        streaming = "";
        busy = false;
        if ((res as any)?.tts_streamed || (res as any)?.ttsStreamed) {
          avatarState = "speaking";
        } else {
          void (async () => {
            try {
              const fresh = await invoke<any>("get_config");
              voiceCfg = fresh?.voice ?? voiceCfg;
              if (!voiceCfg?.enabled || voiceCfg?.auto_read_replies === false) {
                avatarState = "idle";
                return;
              }
              startReadingReply(reply, voiceCfg, invoke, {
                onStart: () => {
                  avatarState = "speaking";
                },
                onDone: () => {
                  if (avatarState === "speaking") avatarState = "idle";
                },
                onError: (err) => {
                  error = `TTS: ${err}`;
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
    } catch (e) {
      error = String(e);
      avatarState = "error";
    } finally {
      busy = false;
    }
  }

  async function send() {
    const text = input.trim();
    if (!text || busy) return;
    input = "";
    await sendText(text);
  }

  async function approveMcpChat() {
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
      pendingItems = res.pending_memory ?? pendingItems;
      mcpPending = res.pending_mcp ?? null;
      pendingQuestion = res.pending_question ?? null;
      const reply = (res.reply || streaming || "").trim();
      if (reply) {
        messages = [...messages, { role: "assistant", content: reply }];
        streaming = "";
        if ((res as any)?.tts_streamed || (res as any)?.ttsStreamed) {
          avatarState = "speaking";
        } else {
          avatarState = "idle";
        }
      } else {
        avatarState = "idle";
      }
      await refreshSessions();
    } catch (e) {
      // Fallback: chat text path
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
    try {
      await invoke("reject_pending_mcp");
    } catch {
      await sendText("rifiuta mcp");
    }
    mcpPending = null;
  }

  function pickOption(option: string) {
    void sendText(option);
  }

  async function listenMic() {
    if (listening || busy || !voiceCfg?.stt_enabled) return;
    listening = true;
    try {
      const text = await invoke<string>("listen_transcribe", { seconds: 5 });
      if (text?.trim()) input = (input ? input + " " : "") + text.trim();
    } catch (e) {
      error = String(e);
    } finally {
      listening = false;
    }
  }

  async function closePanel() {
    await invoke("hide_chat_panel");
  }

  async function openConsole() {
    await invoke("open_console");
  }

  async function newSession() {
    const s = await invoke<any>("new_session");
    sessionId = s.id;
    messages = [];
    streaming = "";
    pendingQuestion = null;
    mcpPending = null;
    showSessions = false;
    await refreshSessions();
  }

  async function openSession(id: string) {
    if (id === sessionId) {
      showSessions = false;
      return;
    }
    await invoke("set_active_session", { sessionId: id });
    sessionId = id;
    const rows = await invoke<any[]>("list_messages", { sessionId: id });
    messages = rows.map((r) => ({ role: r.role, content: r.content }));
    streaming = "";
    pendingQuestion = null;
    showSessions = false;
  }

  function ackAlert() {
    alertMsg = "";
    if (avatarState === "notification") avatarState = "idle";
    void emit("reminder-acked");
  }

  async function approveMem(id: number) {
    try {
      await invoke("approve_memory", { id });
      pendingItems = await invoke("list_pending_memory");
    } catch (e) {
      error = String(e);
    }
  }

  async function rejectMem(id: number) {
    try {
      await invoke("reject_memory", { id });
      pendingItems = await invoke("list_pending_memory");
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="panel">
  <header class="bar" data-tauri-drag-region>
    <div class="who">
      {#key avatarSkin}
        <Avatar2D state={alertMsg ? "notification" : avatarState} size="sm" skin={avatarSkin} />
      {/key}
      <div>
        <strong>AgentOS</strong>
        <span class="badge">{badge}</span>
      </div>
    </div>
    <div class="actions">
      <button class="ghost" type="button" onclick={() => stopSpeaking(invoke)} title="Ferma voce"
        >🔇</button
      >
      <button
        class="ghost"
        type="button"
        onclick={() => {
          showSessions = !showSessions;
          if (showSessions) void refreshSessions();
        }}
        title="Cronologia"
        >☰</button
      >
      <button class="ghost" type="button" onclick={newSession} title="Nuova sessione">＋</button>
      <button class="ghost" type="button" onclick={openConsole} title="Console">⚙</button>
      <button class="ghost" type="button" onclick={closePanel} title="Chiudi">✕</button>
    </div>
  </header>

  {#if showSessions}
    <div class="sessions-rail">
      <div class="sessions-head">
        <strong>Cronologia</strong>
        <button class="ghost" type="button" onclick={newSession}>Nuova</button>
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
          <p class="muted">Nessuna sessione ancora.</p>
        {/each}
      </div>
    </div>
  {/if}

  {#if error}
    <div class="banner">{error}</div>
  {/if}
  {#if pendingItems.length}
    {#each pendingItems as p}
      <div class="banner soft alert-banner">
        <span>
          Salvare in {p.store}: {p.content ?? p.old_text ?? p.action}
        </span>
        <span class="banner-actions">
          <button type="button" onclick={() => approveMem(p.id)}>Approva</button>
          <button class="ghost" type="button" onclick={() => rejectMem(p.id)}>Rifiuta</button>
        </span>
      </div>
    {/each}
  {/if}
  {#if mcpPending}
    <div class="mcp-approve-card">
      <div class="mcp-approve-head">
        <strong>Approvazione MCP</strong>
        <span class="mcp-level">{mcpPending.level}</span>
      </div>
      <p class="mcp-tool">
        <code>{mcpPending.server}/{mcpPending.tool}</code>
      </p>
      {#if mcpPending.arguments != null}
        <pre class="mcp-args">{summarizeArgs(mcpPending.arguments)}</pre>
      {/if}
      <div class="banner-actions">
        <button type="button" disabled={busy} onclick={approveMcpChat}>Approva e continua</button>
        <button class="ghost" type="button" disabled={busy} onclick={rejectMcpChat}>Rifiuta</button>
      </div>
    </div>
  {/if}
  {#if pendingQuestion}
    <div class="banner soft choice-banner">
      <span class="choice-q">{pendingQuestion.question}</span>
      <div class="choice-chips">
        {#each pendingQuestion.options as opt}
          <button type="button" disabled={busy} onclick={() => pickOption(opt)}>{opt}</button>
        {/each}
      </div>
    </div>
  {/if}
  {#if alertMsg}
    <div class="banner soft alert-banner">
      <span>Promemoria: {alertMsg}</span>
      <button class="ghost" type="button" onclick={ackAlert}>Ok, ci penso</button>
    </div>
  {/if}

  <section class="chat">
    {#if messages.length === 0 && !streaming && !busy}
      <p class="empty">Ciao — clicca sull’avatar o scrivi qui per parlare con me.</p>
    {/if}
    {#each messages as m}
      <article class:user={m.role === "user"} class:assistant={m.role === "assistant"}>
        <span class="role">{m.role === "assistant" ? "AgentOS" : "tu"}</span>
        {#if m.role === "assistant"}
          <MarkdownBody source={m.content} />
        {:else}
          <p>{m.content}</p>
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
        <p>{chatStatus || "Sto elaborando…"}</p>
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
      class="ghost"
      disabled={listening || busy || !voiceCfg?.stt_enabled}
      title="Microfono"
      onclick={listenMic}
    >
      {listening ? "…" : "🎙"}
    </button>
    <button type="submit" disabled={busy}>Invia</button>
  </form>
</div>

<style>
  :global(html.shell-chat),
  :global(html.shell-chat body),
  :global(html.shell-chat #boot) {
    background: transparent !important;
    overflow: hidden;
  }

  .panel {
    display: flex;
    flex-direction: column;
    height: 100vh;
    margin: 8px;
    border-radius: 18px;
    border: 1px solid color-mix(in srgb, var(--accent) 28%, var(--line));
    background:
      radial-gradient(420px 220px at 10% -10%, var(--accent-soft), transparent 55%),
      color-mix(in srgb, var(--panel-bg) 95%, transparent);
    box-shadow: var(--shadow);
    overflow: hidden;
    color: var(--ink);
  }

  .bar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
    padding: 0.65rem 0.75rem;
    border-bottom: 1px solid var(--line);
    cursor: grab;
  }

  .who {
    display: flex;
    align-items: center;
    gap: 0.55rem;
  }

  .who strong {
    display: block;
    font-family: var(--font-display);
    font-weight: 500;
    font-size: 0.95rem;
  }

  .badge {
    display: inline-block;
    margin-top: 0.1rem;
    font-size: 0.7rem;
    color: var(--accent);
  }

  .actions {
    display: flex;
    gap: 0.25rem;
  }

  .actions button,
  .composer button {
    border: 1px solid var(--line);
    background: var(--bg2);
    color: var(--ink);
    border-radius: 10px;
    padding: 0.4rem 0.65rem;
    cursor: pointer;
  }

  .ghost {
    background: transparent !important;
    min-width: 2rem;
  }

  .banner {
    margin: 0.55rem 0.75rem 0;
    padding: 0.55rem 0.7rem;
    border-radius: 10px;
    background: rgba(217, 119, 108, 0.15);
    border: 1px solid rgba(217, 119, 108, 0.4);
    font-size: 0.82rem;
  }

  .banner.soft {
    background: var(--warn-bg);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .alert-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.6rem;
    border-color: color-mix(in srgb, #7ec8ff 45%, transparent);
  }

  .alert-banner .ghost {
    flex-shrink: 0;
  }

  .banner-actions {
    display: flex;
    gap: 0.35rem;
    flex-shrink: 0;
  }

  .banner-actions button {
    border: 1px solid var(--line);
    background: var(--bg2);
    color: var(--ink);
    border-radius: 10px;
    padding: 0.3rem 0.65rem;
    cursor: pointer;
    font-size: 0.75rem;
  }

  .choice-banner {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 0.55rem;
  }

  .choice-q {
    font-size: 0.85rem;
    line-height: 1.35;
  }

  .choice-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .choice-chips button {
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--line));
    background: color-mix(in srgb, var(--accent-soft) 55%, var(--bg2));
    color: var(--ink);
    border-radius: 999px;
    padding: 0.4rem 0.85rem;
    cursor: pointer;
    font-size: 0.8rem;
  }

  .choice-chips button:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .chat {
    flex: 1;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 0.65rem;
    padding: 0.75rem;
  }

  .empty {
    margin: auto;
    text-align: center;
    color: var(--muted);
    font-size: 0.9rem;
    line-height: 1.4;
    max-width: 16rem;
  }

  article {
    max-width: 92%;
    padding: 0.7rem 0.85rem;
    border-radius: 14px;
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
  }

  .role {
    display: block;
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
    margin-bottom: 0.2rem;
  }

  article > p {
    margin: 0;
    white-space: pre-wrap;
    line-height: 1.45;
    font-size: 0.92rem;
  }

  .composer {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: 0.5rem;
    padding: 0.65rem 0.75rem 0.8rem;
    border-top: 1px solid var(--line);
  }

  textarea {
    width: 100%;
    border-radius: 12px;
    border: 1px solid var(--line);
    background: color-mix(in srgb, var(--bg0) 90%, var(--bg2));
    color: var(--ink);
    padding: 0.65rem 0.75rem;
    resize: none;
    font: inherit;
  }

  .sessions-rail {
    border-bottom: 1px solid var(--line);
    background: color-mix(in srgb, var(--bg0) 70%, var(--bg2));
    max-height: 11rem;
    display: flex;
    flex-direction: column;
  }

  .sessions-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.45rem 0.75rem;
    font-size: 0.8rem;
  }

  .sessions-list {
    overflow: auto;
    padding: 0.25rem 0.5rem 0.55rem;
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
    padding: 0.4rem 0.55rem;
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
    margin: 0.5rem 0.75rem 0;
    padding: 0.7rem 0.85rem;
    border-radius: 12px;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--line));
    background: color-mix(in srgb, var(--accent-soft) 45%, var(--bg2));
  }

  .mcp-approve-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    margin-bottom: 0.35rem;
  }

  .mcp-level {
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  .mcp-tool {
    margin: 0 0 0.4rem;
    font-size: 0.88rem;
  }

  .mcp-args {
    margin: 0 0 0.55rem;
    padding: 0.45rem 0.55rem;
    max-height: 7rem;
    overflow: auto;
    font-size: 0.75rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: color-mix(in srgb, var(--bg0) 80%, transparent);
    white-space: pre-wrap;
  }

  .muted {
    color: var(--muted);
    font-size: 0.82rem;
    padding: 0.35rem 0.25rem;
  }
</style>
