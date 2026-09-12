<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type McpServer = {
    name: string;
    transport?: string;
    command?: string;
    args?: string[];
    url?: string;
    enabled?: boolean;
    connected?: boolean;
    error?: string | null;
    authenticated?: boolean | null;
  };

  type McpTool = {
    server: string;
    name: string;
    description: string;
  };

  type Pending = {
    server: string;
    tool: string;
    arguments?: unknown;
    level: string;
  };

  let {
    config = null,
    mcpServers = [],
    mcpTools = [],
    mcpStatus = null,
    mcpPending = null,
    sessionId = null,
    onConfig,
    onPending,
    onLedger,
    onApproveChat,
    onRejectChat,
    onRefreshDone,
  }: {
    config?: any;
    mcpServers?: McpServer[];
    mcpTools?: McpTool[];
    mcpStatus?: { servers?: McpServer[] } | null;
    mcpPending?: Pending | null;
    sessionId?: string | null;
    onConfig?: (cfg: any) => void;
    onPending?: (p: Pending | null) => void;
    onLedger?: () => void | Promise<void>;
    onApproveChat?: () => void | Promise<void>;
    onRejectChat?: () => void | Promise<void>;
    onRefreshDone?: (
      tools: McpTool[],
      status: any,
      servers: any[],
    ) => void | Promise<void>;
  } = $props();

  let busy = $state(false);
  let notice = $state("");
  let toolFilter = $state("");
  let showAdvanced = $state(false);
  let showWatch = $state(false);

  let mcpName = $state("");
  let mcpCommand = $state("python3");
  let mcpArgs = $state("");
  let mcpTransport = $state("stdio");
  let mcpUrl = $state("");
  let mcpOauthClientId = $state("");
  let mcpOauthClientSecret = $state("");
  let mcpOauthAuthUrl = $state("");
  let mcpOauthTokenUrl = $state("");
  let mcpOauthScopes = $state("");
  let mcpWatchHint = $state("");
  let mcpWatchTitle = $state("");
  let mcpWatchInterval = $state(300);
  let mcpWatchUpcoming = $state(0);

  let mcpCallServer = $state("echo");
  let mcpCallTool = $state("echo");
  let mcpCallArgs = $state('{"text":"ciao da AgentOS"}');
  let mcpResult = $state("");

  const servers = $derived(
    (mcpStatus?.servers?.length ? mcpStatus.servers : mcpServers) as McpServer[],
  );

  const filteredTools = $derived(
    mcpTools.filter((t) => {
      const q = toolFilter.trim().toLowerCase();
      if (!q) return true;
      return (
        t.server.toLowerCase().includes(q) ||
        t.name.toLowerCase().includes(q) ||
        (t.description || "").toLowerCase().includes(q)
      );
    }),
  );

  function pillFor(s: McpServer): { label: string; kind: string } {
    if (s.connected) return { label: "connected", kind: "ok" };
    if (s.error) return { label: "errore", kind: "bad" };
    if (s.enabled === false) return { label: "disabilitato", kind: "muted" };
    return { label: "offline", kind: "muted" };
  }

  function summarizeArgs(args: unknown): string {
    try {
      const s = JSON.stringify(args ?? {}, null, 2);
      return s.length > 400 ? s.slice(0, 400) + "…" : s;
    } catch {
      return String(args ?? "");
    }
  }

  async function saveConfig(next: any) {
    await invoke("save_app_config", { cfg: next });
    onConfig?.(next);
  }

  export async function doReconnect() {
    busy = true;
    notice = "";
    mcpResult = "";
    try {
      const tools = await invoke<McpTool[]>("refresh_mcp");
      const status = await invoke<any>("mcp_status");
      const pending = await invoke<Pending | null>("get_pending_mcp");
      const list = await invoke<any[]>("list_mcp_servers");
      onPending?.(pending);
      const cfg = await invoke<any>("get_config");
      cfg.mcp_servers = list;
      onConfig?.(cfg);
      await onRefreshDone?.(tools, status, list);
      notice = `Riconnesso · ${tools.length} tool`;
    } catch (e) {
      notice = String(e);
      try {
        const status = await invoke("mcp_status");
        const list = await invoke<any[]>("list_mcp_servers");
        await onRefreshDone?.([], status, list);
      } catch {
        /* ignore */
      }
    } finally {
      busy = false;
    }
  }

  async function addMcpServer() {
    if (!config) {
      notice = "Config non caricata";
      return;
    }
    const args = mcpArgs
      .split(" ")
      .map((s) => s.trim())
      .filter(Boolean);
    const oauth =
      mcpOauthClientId.trim() && mcpOauthAuthUrl.trim() && mcpOauthTokenUrl.trim()
        ? {
            client_id: mcpOauthClientId.trim(),
            client_secret: mcpOauthClientSecret.trim(),
            auth_url: mcpOauthAuthUrl.trim(),
            token_url: mcpOauthTokenUrl.trim(),
            scopes: mcpOauthScopes
              .split(/[,\s]+/)
              .map((s) => s.trim())
              .filter(Boolean),
            extra_auth_params: mcpOauthAuthUrl.includes("google")
              ? { access_type: "offline", prompt: "consent" }
              : {},
          }
        : null;
    const watches =
      showWatch && mcpWatchHint.trim()
        ? [
            {
              name: mcpWatchTitle.trim() || mcpWatchHint.trim(),
              tool: "",
              tool_hint: mcpWatchHint.trim(),
              arguments: {},
              interval_secs: Number(mcpWatchInterval) || 300,
              title: mcpWatchTitle.trim() || mcpWatchHint.trim(),
              notify_on_change: true,
              upcoming_minutes: Number(mcpWatchUpcoming) || 0,
              enabled: true,
            },
          ]
        : [];
    const next = {
      ...config,
      mcp_servers: [
        ...(config.mcp_servers ?? []),
        {
          name: mcpName || "mcp",
          transport: mcpTransport || "stdio",
          command: mcpCommand,
          args,
          env: {},
          url: mcpUrl.trim(),
          headers: {},
          oauth,
          watches,
          enabled: true,
        },
      ],
    };
    await saveConfig(next);
    mcpName = "";
    notice =
      "Server salvato. Premi Riconnetti" + (oauth ? ", poi Accedi se serve OAuth." : ".");
  }

  function applyGmailPreset() {
    mcpName = "gmail";
    mcpTransport = "http";
    mcpCommand = "";
    mcpArgs = "";
    mcpUrl = "https://gmailmcp.googleapis.com/mcp/v1";
    mcpOauthAuthUrl = "https://accounts.google.com/o/oauth2/v2/auth";
    mcpOauthTokenUrl = "https://oauth2.googleapis.com/token";
    mcpOauthScopes = "https://www.googleapis.com/auth/gmail.readonly";
    mcpWatchHint = "message";
    mcpWatchTitle = "Nuove email";
    mcpWatchInterval = 300;
    mcpWatchUpcoming = 0;
    showWatch = true;
  }

  function applyCalendarPreset() {
    mcpName = "calendar";
    mcpTransport = "http";
    mcpCommand = "";
    mcpArgs = "";
    mcpUrl = "https://calendarmcp.googleapis.com/mcp/v1";
    mcpOauthAuthUrl = "https://accounts.google.com/o/oauth2/v2/auth";
    mcpOauthTokenUrl = "https://oauth2.googleapis.com/token";
    mcpOauthScopes =
      "https://www.googleapis.com/auth/calendar.readonly https://www.googleapis.com/auth/calendar.events.readonly";
    mcpWatchHint = "event";
    mcpWatchTitle = "Prossimi impegni";
    mcpWatchInterval = 120;
    mcpWatchUpcoming = 15;
    showWatch = true;
  }

  async function loginMcp(name: string) {
    busy = true;
    notice = "";
    try {
      notice = await invoke<string>("start_mcp_oauth", { name });
      await doReconnect();
    } catch (e) {
      notice = String(e);
      busy = false;
    }
  }

  async function logoutMcp(name: string) {
    try {
      notice = await invoke<string>("logout_mcp_oauth", { name });
      await doReconnect();
    } catch (e) {
      notice = String(e);
    }
  }

  async function removeMcp(name: string) {
    await invoke("remove_mcp_server", { name });
    await doReconnect();
    notice = `Rimosso «${name}»`;
  }

  async function installEchoMcp() {
    busy = true;
    notice = "";
    try {
      const echo = await invoke<any>("install_mcp_echo");
      const cfg = await invoke("get_config");
      onConfig?.(cfg);
      const tools = await invoke<McpTool[]>("refresh_mcp");
      const status = await invoke("mcp_status");
      await onRefreshDone?.(tools, status, (cfg as any).mcp_servers ?? []);
      mcpCallServer = echo.name || "echo";
      notice = `Echo installato · tool: ${tools.map((t) => t.name).join(", ") || "(nessuno)"}`;
      showAdvanced = true;
    } catch (e) {
      notice = String(e);
    } finally {
      busy = false;
    }
  }

  async function runMcpCall(approved = false) {
    busy = true;
    mcpResult = "";
    try {
      let args: Record<string, unknown> = {};
      try {
        args = JSON.parse(mcpCallArgs || "{}");
      } catch {
        args = { text: mcpCallArgs };
      }
      const res = await invoke("call_mcp_tool", {
        server: mcpCallServer,
        tool: mcpCallTool,
        arguments: args,
        approved,
      });
      mcpResult = JSON.stringify(res, null, 2);
      onPending?.(null);
      await onLedger?.();
      notice = "Chiamata eseguita";
    } catch (e) {
      const msg = String(e);
      mcpResult = msg;
      if (msg.includes("APPROVAL_REQUIRED")) {
        onPending?.(await invoke("get_pending_mcp"));
      }
      await onLedger?.();
    } finally {
      busy = false;
    }
  }

  async function approveMcp() {
    if (sessionId && onApproveChat) {
      await onApproveChat();
      return;
    }
    busy = true;
    try {
      try {
        const res = await invoke<any>("approve_and_resume_mcp");
        mcpResult =
          typeof res?.reply === "string" ? res.reply : JSON.stringify(res, null, 2);
      } catch {
        const res = await invoke("approve_pending_mcp");
        mcpResult = JSON.stringify(res, null, 2);
      }
      onPending?.(null);
      await onLedger?.();
      notice = "Approvato";
    } catch (e) {
      notice = String(e);
    } finally {
      busy = false;
    }
  }

  async function rejectMcp() {
    if (sessionId && onRejectChat) {
      await onRejectChat();
      return;
    }
    await invoke("reject_pending_mcp");
    onPending?.(null);
    notice = "Chiamata rifiutata";
  }

  function pickTool(t: McpTool) {
    mcpCallServer = t.server;
    mcpCallTool = t.name;
    showAdvanced = true;
  }
</script>

{#if notice}
  <p class="mcp-notice">{notice}</p>
{/if}

<div class="mcp-root">
  {#if mcpPending}
    <section class="panel warn full-width">
      <h3>Approvazione richiesta</h3>
      <p class="panel-desc">Controlla l’azione prima di eseguirla.</p>
      <p>
        <code>{mcpPending.server}/{mcpPending.tool}</code>
        · livello {mcpPending.level}
      </p>
      <pre class="args-pre">{summarizeArgs(mcpPending.arguments)}</pre>
      <div class="actions">
        <button disabled={busy} onclick={approveMcp}>Approva ed esegui</button>
        <button class="ghost" disabled={busy} onclick={rejectMcp}>Rifiuta</button>
      </div>
    </section>
  {/if}

  <div class="mcp-columns">
    <div class="mcp-main">
      <section class="panel">
        <div class="mcp-head">
          <h3>Stato server</h3>
          <button class="ghost" disabled={busy} onclick={doReconnect}>Riconnetti</button>
        </div>
        <p class="panel-desc">Server collegati e autenticazione.</p>
        {#if servers?.length}
          {#each servers as s}
            {@const pill = pillFor(s)}
            <div class="mcp-row">
              <div class="mcp-body">
                <div class="mcp-titleline">
                  <strong>{s.name}</strong>
                  <span class="mcp-pill" class:ok={pill.kind === "ok"} class:bad={pill.kind === "bad"}
                    >{pill.label}</span
                  >
                  {#if s.authenticated === true}
                    <span class="mcp-pill ok">autenticato</span>
                  {:else if s.authenticated === false}
                    <span class="mcp-pill bad">login richiesto</span>
                  {/if}
                  <span class="mcp-pill muted">{s.transport || "stdio"}</span>
                </div>
                <p class="muted tiny">
                  {s.command ||
                    s.url ||
                    [s.command, ...(s.args || [])].filter(Boolean).join(" ")}
                </p>
                {#if s.error}
                  <p class="err-line">{s.error}</p>
                {/if}
              </div>
              <div class="mcp-actions">
                {#if s.authenticated === false}
                  <button class="ghost" disabled={busy} onclick={() => loginMcp(s.name)}
                    >Accedi</button
                  >
                {:else if s.authenticated === true}
                  <button class="ghost" onclick={() => logoutMcp(s.name)}>Esci</button>
                {/if}
                <button class="ghost" onclick={() => removeMcp(s.name)}>Rimuovi</button>
              </div>
            </div>
          {/each}
        {:else}
          <p class="muted">Nessun server. Aggiungine uno a destra, oppure usa un preset.</p>
        {/if}
      </section>

      <section class="panel">
        <div class="mcp-head">
          <h3>Tool scoperti</h3>
          <input class="filter" placeholder="Filtra…" bind:value={toolFilter} />
        </div>
        <p class="panel-desc">Clicca un tool per aprirlo in Smoke (avanzate).</p>
        {#each filteredTools as t}
          <div class="mcp-row tool">
            <div class="mcp-body">
              <button class="ghost tool-btn" type="button" onclick={() => pickTool(t)}>
                <code>{t.server}/{t.name}</code>
              </button>
              <p class="muted tiny">{t.description || "—"}</p>
            </div>
          </div>
        {:else}
          <p class="muted">Nessun tool. Premi Riconnetti o collega un server.</p>
        {/each}
      </section>
    </div>

    <aside class="mcp-side">
      <section class="panel">
        <h3>Aggiungi server</h3>
        <p class="panel-desc">Preset o configurazione manuale (stdio / HTTP + OAuth).</p>
        <div class="preset-row">
          <button class="ghost" type="button" onclick={applyGmailPreset}>Gmail</button>
          <button class="ghost" type="button" onclick={applyCalendarPreset}>Calendar</button>
        </div>
        <div class="stack">
          <input placeholder="nome (univoco)" bind:value={mcpName} />
          <label class="muted"
            >Trasporto
            <select bind:value={mcpTransport}>
              <option value="stdio">stdio (locale)</option>
              <option value="http">http (remoto)</option>
            </select>
          </label>
          {#if mcpTransport === "stdio"}
            <input placeholder="comando (npx, python3…)" bind:value={mcpCommand} />
            <input placeholder="args separati da spazio" bind:value={mcpArgs} />
          {:else}
            <input placeholder="URL MCP" bind:value={mcpUrl} />
            <input placeholder="OAuth client id" bind:value={mcpOauthClientId} />
            <input
              type="password"
              placeholder="OAuth client secret"
              bind:value={mcpOauthClientSecret}
            />
            <input placeholder="OAuth auth URL" bind:value={mcpOauthAuthUrl} />
            <input placeholder="OAuth token URL" bind:value={mcpOauthTokenUrl} />
            <input placeholder="scopes" bind:value={mcpOauthScopes} />
            <p class="muted tiny">
              Google: client Desktop + redirect
              <code>http://127.0.0.1:&lt;porta&gt;/callback</code>
            </p>
          {/if}
          <label class="check">
            <input type="checkbox" bind:checked={showWatch} />
            Watch / notifiche (opzionale)
          </label>
          {#if showWatch}
            <input placeholder="hint tool (message, event…)" bind:value={mcpWatchHint} />
            <input placeholder="titolo notifica" bind:value={mcpWatchTitle} />
            <label class="muted"
              >Intervallo (s)
              <input type="number" min="30" bind:value={mcpWatchInterval} />
            </label>
            <label class="muted"
              >Avviso imminente (min, 0 = solo cambi)
              <input type="number" min="0" bind:value={mcpWatchUpcoming} />
            </label>
          {/if}
          <button disabled={busy || !config} onclick={addMcpServer}>Salva server</button>
        </div>
      </section>
    </aside>
  </div>

  <section class="panel full-width advanced">
    <button class="ghost adv-toggle" type="button" onclick={() => (showAdvanced = !showAdvanced)}>
      {showAdvanced ? "▼" : "▶"} Avanzate / Smoke test
    </button>
    {#if showAdvanced}
      <p class="panel-desc">Debug: chiama un tool a mano o installa l’echo smoke.</p>
      <div class="actions" style="margin-bottom: 0.5rem;">
        <button class="ghost" disabled={busy} onclick={installEchoMcp}>Installa echo smoke</button>
      </div>
      <div class="inline">
        <input placeholder="server" bind:value={mcpCallServer} />
        <input placeholder="tool" bind:value={mcpCallTool} />
      </div>
      <textarea rows="3" bind:value={mcpCallArgs} placeholder="JSON args"></textarea>
      <div class="actions">
        <button disabled={busy} onclick={() => runMcpCall(false)}>Esegui</button>
      </div>
      {#if mcpResult}
        <pre class="hit">{mcpResult}</pre>
      {/if}
    {/if}
  </section>
</div>

<style>
  .mcp-root {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    min-height: 0;
  }

  .mcp-columns {
    display: grid;
    grid-template-columns: 1.4fr 1fr;
    gap: var(--space-md);
    align-items: start;
  }

  @media (max-width: 960px) {
    .mcp-columns {
      grid-template-columns: 1fr;
    }
  }

  .mcp-main,
  .mcp-side {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    min-width: 0;
  }

  .mcp-notice {
    margin: 0 0 var(--space-sm);
    padding: 0.55rem 0.75rem;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--accent-soft, rgba(91, 141, 239, 0.12));
    color: var(--ink);
    font-size: 0.88rem;
  }

  .mcp-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-sm);
    margin-bottom: 0.25rem;
  }

  .mcp-head h3 {
    margin: 0;
  }

  .filter {
    max-width: 10rem;
    font-size: 0.85rem;
  }

  .mcp-row {
    display: flex;
    justify-content: space-between;
    gap: var(--space-sm);
    padding: 0.55rem 0;
    border-top: 1px solid var(--line);
  }

  .mcp-row:first-of-type {
    border-top: none;
  }

  .mcp-body {
    min-width: 0;
    flex: 1;
  }

  .mcp-titleline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem;
  }

  .mcp-pill {
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0.15rem 0.45rem;
    border-radius: 999px;
    border: 1px solid var(--line);
    color: var(--muted);
    background: transparent;
  }

  .mcp-pill.ok {
    color: var(--success, #3db89a);
    border-color: color-mix(in srgb, var(--success, #3db89a) 40%, var(--line));
    background: var(--success-bg, rgba(61, 184, 154, 0.12));
  }

  .mcp-pill.bad {
    color: #e86b8a;
    border-color: color-mix(in srgb, #e86b8a 40%, var(--line));
    background: rgba(232, 107, 138, 0.1);
  }

  .mcp-pill.muted {
    opacity: 0.85;
  }

  .mcp-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    flex-shrink: 0;
  }

  .tiny {
    font-size: 0.78rem;
    margin: 0.2rem 0 0;
  }

  .err-line {
    margin: 0.25rem 0 0;
    font-size: 0.8rem;
    color: #e86b8a;
  }

  .tool-btn {
    padding: 0;
    text-align: left;
  }

  .preset-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-bottom: 0.55rem;
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }

  .inline {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    margin-bottom: var(--space-sm);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-size: 0.88rem;
    color: var(--muted);
  }

  .args-pre,
  .hit {
    font-size: 0.78rem;
    overflow: auto;
    max-height: 12rem;
    padding: 0.5rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: rgba(0, 0, 0, 0.25);
  }

  .advanced {
    margin-top: 0.25rem;
  }

  .adv-toggle {
    width: 100%;
    text-align: left;
    font-weight: 600;
  }

  .full-width {
    width: 100%;
  }
</style>
