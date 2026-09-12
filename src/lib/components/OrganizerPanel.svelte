<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type Label = {
    id: number;
    name: string;
    kind: string;
    color: string;
    created_at: string;
  };

  type Task = {
    id: number;
    title: string;
    description: string;
    status: string;
    priority: number;
    deadline: string | null;
    created_at: string;
    updated_at: string;
    labels?: Label[];
  };

  type Reminder = {
    id: number;
    type: string;
    schedule: string;
    message: string;
    enabled: boolean;
    fired: boolean;
    created_at: string;
    task_id?: number | null;
    recurrence?: string;
  };

  type MainTab = "board" | "today" | "agenda";
  type ColumnMode = "status" | "client" | "priority";

  let {
    tasks = [],
    reminders = [],
    labels = [],
    onChanged,
  }: {
    tasks?: Task[];
    reminders?: Reminder[];
    labels?: Label[];
    onChanged?: () => void | Promise<void>;
  } = $props();

  let mainTab = $state<MainTab>("board");
  let columnMode = $state<ColumnMode>("status");
  let filterLabelId = $state<number | null>(null);
  let doneCollapsed = $state(false);
  let doneLimit = $state(8);

  let newTitle = $state("");
  let newPriority = $state(2);
  let newDeadline = $state("");
  let newLabelIds = $state<number[]>([]);

  let remSchedule = $state("");
  let remMessage = $state("");
  let remTaskId = $state("");
  let remRecurrence = $state("none");

  let editingId = $state<number | null>(null);
  let editTitle = $state("");
  let editPriority = $state(2);
  let editDeadline = $state("");
  let editLabelIds = $state<number[]>([]);
  let menuTaskId = $state<number | null>(null);
  let menuRemId = $state<number | null>(null);

  let showLabelDrawer = $state(false);
  let newLabelName = $state("");
  let newLabelKind = $state("tag");
  let newLabelColor = $state("#5b8def");
  let labelQuery = $state("");
  let colorEditId = $state<number | null>(null);

  let busy = $state(false);
  let notice = $state("");

  const LABEL_COLORS = [
    "#5b8def",
    "#3db89a",
    "#e5a84a",
    "#e86b8a",
    "#9b7bff",
    "#4ecdc4",
    "#f0734a",
    "#6bcb77",
    "#c77dff",
    "#4d96ff",
    "#ff6b6b",
    "#20c997",
    "#fdae4b",
    "#845ef7",
    "#22b8cf",
    "#f06595",
  ];

  const DONE_PREVIEW = 8;

  function refresh() {
    return onChanged?.();
  }

  function parseWhen(iso: string | null | undefined): Date | null {
    if (!iso) return null;
    const d = new Date(iso);
    if (!Number.isNaN(d.getTime())) return d;
    const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})(?::(\d{2}))?/);
    if (!m) return null;
    return new Date(
      Number(m[1]),
      Number(m[2]) - 1,
      Number(m[3]),
      Number(m[4]),
      Number(m[5]),
      Number(m[6] ?? 0),
    );
  }

  function startOfDay(d: Date) {
    return new Date(d.getFullYear(), d.getMonth(), d.getDate());
  }

  function humanWhen(iso: string | null | undefined): string {
    const d = parseWhen(iso);
    if (!d) return iso ?? "";
    const now = new Date();
    const today = startOfDay(now);
    const that = startOfDay(d);
    const diffDays = Math.round((that.getTime() - today.getTime()) / 86_400_000);
    const hm = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (diffDays === 0) return `oggi ${hm}`;
    if (diffDays === 1) return `domani ${hm}`;
    if (diffDays === -1) return `ieri ${hm}`;
    if (diffDays > 1 && diffDays < 7) {
      const wd = d.toLocaleDateString([], { weekday: "short" });
      return `${wd} ${hm}`;
    }
    return d.toLocaleString([], {
      day: "2-digit",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function prioLabel(p: number) {
    if (p === 1) return "Alta";
    if (p === 3) return "Bassa";
    return "Media";
  }

  function kindLabel(k: string) {
    if (k === "project") return "Progetto";
    if (k === "client") return "Cliente";
    if (k === "area") return "Area";
    return "Tag";
  }

  function recurrenceLabel(r: string | undefined) {
    if (r === "daily") return "Ogni giorno";
    if (r === "weekly") return "Ogni settimana";
    if (r === "monthly") return "Ogni mese";
    return "Una tantum";
  }

  function toLocalInput(iso: string | null | undefined): string {
    const d = parseWhen(iso);
    if (!d) return "";
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  function fromLocalInput(v: string): string | null {
    const t = v.trim();
    if (!t) return null;
    const d = new Date(t);
    if (Number.isNaN(d.getTime())) return null;
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  function toggleId(list: number[], id: number) {
    return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
  }

  function taskTitle(id: number | null | undefined) {
    if (id == null) return null;
    return tasks.find((t) => t.id === id)?.title ?? `task #${id}`;
  }

  function matchesFilter(t: Task) {
    if (filterLabelId == null) return true;
    return (t.labels ?? []).some((l) => l.id === filterLabelId);
  }

  const filteredTasks = $derived(tasks.filter(matchesFilter));
  const openTasks = $derived(filteredTasks.filter((t) => t.status === "open"));
  const doingTasks = $derived(filteredTasks.filter((t) => t.status === "doing"));
  const doneTasks = $derived(filteredTasks.filter((t) => t.status === "done"));

  const activeForLink = $derived(
    tasks.filter((t) => t.status === "open" || t.status === "doing"),
  );

  type BoardCol = { key: string; title: string; items: Task[]; collapsible?: boolean; isDone?: boolean };

  const boardColumns = $derived.by((): BoardCol[] => {
    if (columnMode === "status") {
      return [
        { key: "open", title: "Da fare", items: openTasks },
        { key: "doing", title: "In corso", items: doingTasks },
        {
          key: "done",
          title: "Fatto",
          items: doneTasks,
          collapsible: true,
          isDone: true,
        },
      ];
    }
    if (columnMode === "priority") {
      return [1, 2, 3].map((p) => ({
        key: `p${p}`,
        title: `Priorità ${prioLabel(p)}`,
        items: filteredTasks.filter(
          (t) => t.priority === p && t.status !== "done",
        ),
      }));
    }
    // client columns
    const clients = labels.filter((l) => l.kind === "client");
    const cols: BoardCol[] = clients.map((c) => ({
      key: `c${c.id}`,
      title: c.name,
      items: filteredTasks.filter(
        (t) =>
          t.status !== "done" && (t.labels ?? []).some((l) => l.id === c.id),
      ),
    }));
    cols.push({
      key: "noclient",
      title: "Senza cliente",
      items: filteredTasks.filter(
        (t) =>
          t.status !== "done" &&
          !(t.labels ?? []).some((l) => l.kind === "client"),
      ),
    });
    return cols;
  });

  const todayFocus = $derived.by(() => {
    const now = new Date();
    const sod = startOfDay(now);
    const eod = new Date(sod.getTime() + 86_400_000);
    const active = tasks.filter((t) => t.status === "open" || t.status === "doing");
    const overdue: Task[] = [];
    const dueToday: Task[] = [];
    const doing: Task[] = [];
    for (const t of active) {
      if (t.status === "doing") doing.push(t);
      const dl = parseWhen(t.deadline);
      if (!dl) continue;
      if (dl < sod) overdue.push(t);
      else if (dl < eod) dueToday.push(t);
    }
    const remToday = reminders.filter((r) => {
      if (r.fired || !r.enabled) return false;
      const w = parseWhen(r.schedule);
      return !!w && w >= sod && w < eod;
    });
    return { overdue, dueToday, doing, remToday };
  });

  const upcomingReminders = $derived(
    reminders
      .filter((r) => !r.fired && r.enabled)
      .slice()
      .sort((a, b) => {
        const da = parseWhen(a.schedule)?.getTime() ?? 0;
        const db = parseWhen(b.schedule)?.getTime() ?? 0;
        return da - db;
      }),
  );

  const filterLabels = $derived(
    labels.slice().sort((a, b) => a.name.localeCompare(b.name)),
  );

  const drawerLabels = $derived.by(() => {
    const q = labelQuery.trim().toLowerCase();
    const list = q
      ? labels.filter(
          (l) =>
            l.name.toLowerCase().includes(q) ||
            l.kind.toLowerCase().includes(q),
        )
      : labels;
    const order = ["client", "project", "area", "tag"];
    const map = new Map<string, Label[]>();
    for (const lab of list) {
      if (!map.has(lab.kind)) map.set(lab.kind, []);
      map.get(lab.kind)!.push(lab);
    }
    return order
      .filter((k) => map.has(k))
      .map((k) => ({
        kind: k,
        title: kindLabel(k),
        items: map.get(k)!.sort((a, b) => a.name.localeCompare(b.name)),
      }));
  });

  function visibleDone(items: Task[]) {
    if (doneCollapsed) return [];
    if (items.length <= doneLimit) return items;
    return items.slice(0, doneLimit);
  }

  async function run(fn: () => Promise<void>) {
    busy = true;
    notice = "";
    try {
      await fn();
      await refresh();
    } catch (e) {
      notice = String(e);
    } finally {
      busy = false;
      menuTaskId = null;
      menuRemId = null;
    }
  }

  async function addTask() {
    if (!newTitle.trim()) return;
    await run(async () => {
      await invoke("create_task", {
        title: newTitle.trim(),
        description: "",
        priority: Number(newPriority) || 2,
        deadline: fromLocalInput(newDeadline),
        labelIds: newLabelIds,
      });
      newTitle = "";
      newDeadline = "";
      newPriority = 2;
      newLabelIds = [];
    });
  }

  async function setStatus(id: number, status: string) {
    await run(async () => {
      await invoke("set_task_status", { id, status });
      if (editingId === id) editingId = null;
    });
  }

  async function removeTask(id: number) {
    await run(async () => {
      await invoke("delete_task", { id });
      if (editingId === id) editingId = null;
    });
  }

  function startEdit(t: Task) {
    editingId = t.id;
    editTitle = t.title;
    editPriority = t.priority;
    editDeadline = toLocalInput(t.deadline);
    editLabelIds = (t.labels ?? []).map((l) => l.id);
    menuTaskId = null;
  }

  async function saveEdit() {
    if (editingId == null || !editTitle.trim()) return;
    const id = editingId;
    await run(async () => {
      await invoke("update_task", {
        id,
        title: editTitle.trim(),
        priority: Number(editPriority) || 2,
        deadline: fromLocalInput(editDeadline) ?? "",
        touchDeadline: true,
        status: null,
        description: null,
      });
      await invoke("set_task_labels", { taskId: id, labelIds: editLabelIds });
      editingId = null;
    });
  }

  async function addReminder() {
    if (!remSchedule.trim()) return;
    await run(async () => {
      await invoke("create_reminder", {
        schedule: remSchedule.trim(),
        message: remMessage.trim() || "Promemoria",
        taskId: remTaskId ? Number(remTaskId) : null,
        recurrence: remRecurrence || "none",
      });
      remSchedule = "";
      remMessage = "";
      remTaskId = "";
      remRecurrence = "none";
    });
  }

  async function removeReminder(id: number) {
    await run(async () => invoke("delete_reminder", { id }));
  }

  async function snooze(id: number, minutes: number) {
    await run(async () => {
      await invoke("snooze_reminder", { id, minutes });
      notice = `Snooze ${minutes} min.`;
    });
  }

  async function snoozeTomorrow(id: number) {
    await run(async () => {
      await invoke("snooze_reminder_tomorrow", { id, hour: 9, minute: 0 });
      notice = "Rimandato a domani alle 09:00.";
    });
  }

  async function quickTest() {
    const d = new Date(Date.now() + 90_000);
    const pad = (n: number) => String(n).padStart(2, "0");
    const schedule = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
    await run(async () => {
      await invoke("create_reminder", {
        schedule,
        message: "Test notifiche AgentOS",
        taskId: null,
        recurrence: "none",
      });
      notice = `Reminder di test (~90s)`;
    });
  }

  async function addLabel() {
    if (!newLabelName.trim()) return;
    await run(async () => {
      await invoke("create_label", {
        name: newLabelName.trim(),
        kind: newLabelKind,
        color: newLabelColor,
      });
      newLabelName = "";
      const used = new Set(labels.map((l) => l.color.toLowerCase()));
      newLabelColor =
        LABEL_COLORS.find((c) => !used.has(c.toLowerCase())) ??
        LABEL_COLORS[labels.length % LABEL_COLORS.length];
      notice = "Etichetta creata.";
    });
  }

  async function removeLabel(id: number) {
    await run(async () => {
      await invoke("delete_label", { id });
      if (filterLabelId === id) filterLabelId = null;
      if (colorEditId === id) colorEditId = null;
    });
  }

  async function setLabelColor(id: number, color: string) {
    await run(async () => {
      await invoke("update_label_color", { id, color });
      colorEditId = null;
    });
  }

  async function redistributeColors() {
    await run(async () => {
      const n = await invoke<number>("recolor_generic_labels");
      notice = n ? `Colori aggiornati su ${n} etichette.` : "Colori già distinti.";
    });
  }

  function onCaptureKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void addTask();
    }
  }

  function closeMenus() {
    menuTaskId = null;
    menuRemId = null;
  }

  function columnItems(col: BoardCol) {
    if (col.isDone) return visibleDone(col.items);
    return col.items;
  }
</script>

<svelte:window
  onclick={() => {
    if (menuTaskId != null || menuRemId != null) closeMenus();
  }}
/>

{#if notice}
  <p class="org-notice">{notice}</p>
{/if}

<div class="org-root">
  <header class="org-top">
    <nav class="org-tabs" aria-label="Viste organizer">
      <button
        type="button"
        class="org-tab"
        class:active={mainTab === "board"}
        onclick={() => (mainTab = "board")}>Bacheca</button
      >
      <button
        type="button"
        class="org-tab"
        class:active={mainTab === "today"}
        onclick={() => (mainTab = "today")}
      >
        Oggi
        {#if todayFocus.overdue.length || todayFocus.dueToday.length || todayFocus.doing.length}
          <span class="org-tab-dot"></span>
        {/if}
      </button>
      <button
        type="button"
        class="org-tab"
        class:active={mainTab === "agenda"}
        onclick={() => (mainTab = "agenda")}
      >
        Agenda
        {#if upcomingReminders.length}
          <span class="org-badge">{upcomingReminders.length}</span>
        {/if}
      </button>
    </nav>
    <div class="org-top-actions">
      {#if mainTab === "board"}
        <select bind:value={columnMode} aria-label="Modalità colonne" disabled={busy}>
          <option value="status">Colonne: stato</option>
          <option value="client">Colonne: cliente</option>
          <option value="priority">Colonne: priorità</option>
        </select>
      {/if}
      <button type="button" class="ghost" onclick={() => (showLabelDrawer = true)} disabled={busy}>
        Gestisci etichette
      </button>
    </div>
  </header>

  {#if mainTab === "board"}
    <div class="org-capture-bar">
      <input
        class="org-capture-input"
        placeholder="Cattura rapida…"
        bind:value={newTitle}
        onkeydown={onCaptureKey}
        disabled={busy}
      />
      <select bind:value={newPriority} disabled={busy} aria-label="Priorità">
        <option value={1}>Alta</option>
        <option value={2}>Media</option>
        <option value={3}>Bassa</option>
      </select>
      <input type="datetime-local" bind:value={newDeadline} disabled={busy} aria-label="Scadenza" />
      <button type="button" onclick={addTask} disabled={busy || !newTitle.trim()}>Aggiungi</button>
    </div>

    {#if filterLabels.length}
      <div class="org-filters" role="toolbar" aria-label="Filtra per etichetta">
        <button
          type="button"
          class="org-filter"
          class:on={filterLabelId == null}
          onclick={() => (filterLabelId = null)}>Tutti</button
        >
        {#each filterLabels as lab}
          <button
            type="button"
            class="org-filter"
            class:on={filterLabelId === lab.id}
            style={`--lab:${lab.color}`}
            onclick={() => (filterLabelId = filterLabelId === lab.id ? null : lab.id)}
            title={`${kindLabel(lab.kind)} · ${lab.name}`}
          >
            <span class="org-dot"></span>
            {lab.name}
          </button>
        {/each}
      </div>
    {/if}

    {#if newLabelIds.length || labels.length}
      <div class="org-assign-row">
        <span class="muted tiny">Assegna alla cattura:</span>
        {#each labels.slice(0, 24) as lab}
          <button
            type="button"
            class="org-filter sm"
            class:on={newLabelIds.includes(lab.id)}
            style={`--lab:${lab.color}`}
            onclick={() => (newLabelIds = toggleId(newLabelIds, lab.id))}
            disabled={busy}
          >
            {lab.name}
          </button>
        {/each}
      </div>
    {/if}

    <div
      class="org-board"
      class:cols-3={columnMode === "status" || columnMode === "priority"}
      class:cols-scroll={columnMode === "client"}
    >
      {#each boardColumns as col}
        <section class="org-col" class:done-col={col.isDone}>
          <header class="org-col-head">
            <h3>{col.title}</h3>
            <span class="org-count">{col.items.length}</span>
            {#if col.collapsible}
              <button
                type="button"
                class="ghost org-collapse"
                onclick={() => (doneCollapsed = !doneCollapsed)}
              >
                {doneCollapsed ? "Espandi" : "Comprimi"}
              </button>
            {/if}
          </header>

          {#if col.isDone && doneCollapsed}
            <p class="muted tiny org-col-empty">{col.items.length} completati (archivio compresso)</p>
          {:else}
            <div class="org-col-body">
              {#each columnItems(col) as t}
                {#if editingId === t.id}
                  <div class="org-card editing">
                    <input bind:value={editTitle} />
                    <div class="org-edit-row">
                      <select bind:value={editPriority}>
                        <option value={1}>Alta</option>
                        <option value={2}>Media</option>
                        <option value={3}>Bassa</option>
                      </select>
                      <input type="datetime-local" bind:value={editDeadline} />
                    </div>
                    <div class="org-assign-row compact">
                      {#each labels as lab}
                        <button
                          type="button"
                          class="org-filter sm"
                          class:on={editLabelIds.includes(lab.id)}
                          style={`--lab:${lab.color}`}
                          onclick={() => (editLabelIds = toggleId(editLabelIds, lab.id))}
                        >
                          {lab.name}
                        </button>
                      {/each}
                    </div>
                    <div class="org-card-actions">
                      <button type="button" onclick={saveEdit} disabled={busy}>Salva</button>
                      <button type="button" class="ghost" onclick={() => (editingId = null)}
                        >Annulla</button
                      >
                    </div>
                  </div>
                {:else}
                  <article
                    class="org-card"
                    class:doing={t.status === "doing"}
                    class:prio-high={t.priority === 1}
                    class:done={t.status === "done"}
                  >
                    <div class="org-card-top">
                      <button
                        type="button"
                        class="org-check"
                        title="Completa"
                        onclick={() => setStatus(t.id, t.status === "done" ? "open" : "done")}
                        disabled={busy}
                      ></button>
                      <button type="button" class="org-card-title" onclick={() => startEdit(t)}>
                        {t.title}
                      </button>
                      <div class="org-menu-wrap">
                        <button
                          type="button"
                          class="org-menu-btn"
                          aria-label="Azioni"
                          onclick={(e) => {
                            e.stopPropagation();
                            menuTaskId = menuTaskId === t.id ? null : t.id;
                          }}
                        >
                          ···
                        </button>
                        {#if menuTaskId === t.id}
                          <!-- svelte-ignore a11y_click_events_have_key_events -->
                          <!-- svelte-ignore a11y_no_static_element_interactions -->
                          <div class="org-menu" onclick={(e) => e.stopPropagation()}>
                            {#if t.status !== "doing" && t.status !== "done"}
                              <button type="button" onclick={() => setStatus(t.id, "doing")}
                                >In corso</button
                              >
                            {/if}
                            {#if t.status === "doing"}
                              <button type="button" onclick={() => setStatus(t.id, "open")}
                                >Da fare</button
                              >
                            {/if}
                            {#if t.status === "done"}
                              <button type="button" onclick={() => setStatus(t.id, "open")}
                                >Riapri</button
                              >
                            {:else}
                              <button type="button" onclick={() => setStatus(t.id, "done")}
                                >Completa</button
                              >
                            {/if}
                            <button type="button" onclick={() => startEdit(t)}>Modifica</button>
                            <button type="button" class="danger" onclick={() => removeTask(t.id)}
                              >Elimina</button
                            >
                          </div>
                        {/if}
                      </div>
                    </div>
                    <div class="org-card-meta">
                      {#if t.deadline}
                        <span
                          class="org-pill"
                          class:late={(() => {
                            const d = parseWhen(t.deadline);
                            return d ? d < startOfDay(new Date()) : false;
                          })()}
                        >
                          {humanWhen(t.deadline)}
                        </span>
                      {/if}
                      {#if t.priority === 1}
                        <span class="org-pill hot">Alta</span>
                      {/if}
                      {#each t.labels ?? [] as lab}
                        <span class="org-pill lab" style={`--lab:${lab.color}`}>#{lab.name}</span>
                      {/each}
                    </div>
                  </article>
                {/if}
              {:else}
                <p class="muted tiny org-col-empty">Nessun task</p>
              {/each}

              {#if col.isDone && col.items.length > doneLimit && !doneCollapsed}
                <button
                  type="button"
                  class="ghost org-more-done"
                  onclick={() => (doneLimit = doneLimit >= col.items.length ? DONE_PREVIEW : col.items.length)}
                >
                  {doneLimit >= col.items.length
                    ? "Mostra meno"
                    : `Mostra altri ${col.items.length - doneLimit}`}
                </button>
              {/if}
            </div>
          {/if}

          {#if !col.isDone}
            <button
              type="button"
              class="org-col-add"
              disabled={busy}
              onclick={() => {
                const el = document.querySelector<HTMLInputElement>(".org-capture-input");
                el?.focus();
              }}
            >
              + Aggiungi
            </button>
          {/if}
        </section>
      {/each}
    </div>
  {:else if mainTab === "today"}
    <section class="org-today panel-like">
      <h3>Focus di oggi</h3>
      <p class="panel-desc">In ritardo, scadenze, in corso e promemoria della giornata.</p>
      <div class="org-today-grid">
        <div class="org-bucket">
          <h4>In ritardo <span class="org-count">{todayFocus.overdue.length}</span></h4>
          {#each todayFocus.overdue as t}
            <button type="button" class="org-chip overdue" onclick={() => { mainTab = "board"; startEdit(t); }}>
              {t.title}
              <span>{humanWhen(t.deadline)}</span>
            </button>
          {:else}
            <p class="muted tiny">Niente in ritardo.</p>
          {/each}
        </div>
        <div class="org-bucket">
          <h4>Scade oggi <span class="org-count">{todayFocus.dueToday.length}</span></h4>
          {#each todayFocus.dueToday as t}
            <button type="button" class="org-chip" onclick={() => { mainTab = "board"; startEdit(t); }}>
              {t.title}
              <span>{humanWhen(t.deadline)}</span>
            </button>
          {:else}
            <p class="muted tiny">Nessuna scadenza oggi.</p>
          {/each}
        </div>
        <div class="org-bucket">
          <h4>In corso <span class="org-count">{todayFocus.doing.length}</span></h4>
          {#each todayFocus.doing as t}
            <button type="button" class="org-chip doing" onclick={() => { mainTab = "board"; startEdit(t); }}>
              {t.title}
            </button>
          {:else}
            <p class="muted tiny">Nessun task in corso.</p>
          {/each}
        </div>
        <div class="org-bucket">
          <h4>Promemoria <span class="org-count">{todayFocus.remToday.length}</span></h4>
          {#each todayFocus.remToday as r}
            <div class="org-chip rem">
              <span>{r.message}</span>
              <span>{humanWhen(r.schedule)}</span>
            </div>
          {:else}
            <p class="muted tiny">Nessun promemoria oggi.</p>
          {/each}
        </div>
      </div>
    </section>
  {:else}
    <section class="org-agenda panel-like">
      <h3>Agenda</h3>
      <p class="panel-desc">Promemoria in ordine temporale. Collegabili a un task, con ricorrenza.</p>
      <div class="org-agenda-form">
        <input placeholder="2026-08-26 09:00" bind:value={remSchedule} disabled={busy} />
        <input placeholder="Messaggio" bind:value={remMessage} disabled={busy} />
        <select bind:value={remTaskId} disabled={busy} aria-label="Task collegato">
          <option value="">Nessun task</option>
          {#each activeForLink as t}
            <option value={String(t.id)}>#{t.id} {t.title}</option>
          {/each}
        </select>
        <select bind:value={remRecurrence} disabled={busy} aria-label="Ricorrenza">
          <option value="none">Una tantum</option>
          <option value="daily">Ogni giorno</option>
          <option value="weekly">Ogni settimana</option>
          <option value="monthly">Ogni mese</option>
        </select>
        <button type="button" onclick={addReminder} disabled={busy || !remSchedule.trim()}
          >Crea</button
        >
        <button type="button" class="ghost" onclick={quickTest} disabled={busy}>Test ~90s</button>
      </div>

      <div class="org-agenda-list">
        {#each upcomingReminders as r}
          <article class="org-agenda-row">
            <div class="org-agenda-main">
              <strong>{r.message}</strong>
              <div class="org-card-meta">
                <span class="org-pill">{humanWhen(r.schedule)}</span>
                <span class="org-pill">{recurrenceLabel(r.recurrence)}</span>
                {#if r.task_id}
                  <span class="org-pill lab">→ {taskTitle(r.task_id)}</span>
                {/if}
              </div>
            </div>
            <div class="org-menu-wrap">
              <button
                type="button"
                class="org-menu-btn"
                aria-label="Azioni promemoria"
                onclick={(e) => {
                  e.stopPropagation();
                  menuRemId = menuRemId === r.id ? null : r.id;
                }}
              >
                ···
              </button>
              {#if menuRemId === r.id}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class="org-menu" onclick={(e) => e.stopPropagation()}>
                  <button type="button" onclick={() => snooze(r.id, 15)}>+15 minuti</button>
                  <button type="button" onclick={() => snooze(r.id, 60)}>+1 ora</button>
                  <button type="button" onclick={() => snoozeTomorrow(r.id)}>Domani 9:00</button>
                  <button type="button" class="danger" onclick={() => removeReminder(r.id)}
                    >Elimina</button
                  >
                </div>
              {/if}
            </div>
          </article>
        {:else}
          <p class="muted">Nessun promemoria attivo.</p>
        {/each}
      </div>
    </section>
  {/if}
</div>

{#if showLabelDrawer}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="org-drawer-backdrop"
    onclick={() => {
      showLabelDrawer = false;
      colorEditId = null;
    }}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <aside class="org-drawer" onclick={(e) => e.stopPropagation()} role="dialog" aria-label="Gestisci etichette">
      <header class="org-drawer-head">
        <h3>Etichette <span class="org-count">{labels.length}</span></h3>
        <button type="button" class="ghost" onclick={() => (showLabelDrawer = false)}>Chiudi</button>
      </header>
      <p class="panel-desc">Crea, colora ed elimina. In bacheca servono solo come filtri.</p>

      <div class="org-drawer-create">
        <input placeholder="Nuova etichetta…" bind:value={newLabelName} disabled={busy} />
        <select bind:value={newLabelKind} disabled={busy}>
          <option value="tag">Tag</option>
          <option value="project">Progetto</option>
          <option value="client">Cliente</option>
          <option value="area">Area</option>
        </select>
        <button type="button" onclick={addLabel} disabled={busy || !newLabelName.trim()}
          >Crea</button
        >
      </div>
      <div class="org-swatches">
        {#each LABEL_COLORS as c}
          <button
            type="button"
            class="org-swatch"
            class:on={newLabelColor.toLowerCase() === c.toLowerCase()}
            style={`--lab:${c}`}
            onclick={() => (newLabelColor = c)}
            disabled={busy}
          ></button>
        {/each}
      </div>
      <div class="org-drawer-tools">
        <input placeholder="Filtra…" bind:value={labelQuery} disabled={busy} />
        <button type="button" class="ghost" onclick={redistributeColors} disabled={busy}
          >Ridistribuisci colori</button
        >
      </div>

      {#each drawerLabels as group}
        <div class="org-drawer-group">
          <h4>{group.title}</h4>
          <div class="org-label-inline">
            {#each group.items as lab}
              <div class="org-chip-lab" class:editing={colorEditId === lab.id} style={`--lab:${lab.color}`}>
                <button
                  type="button"
                  class="org-chip-main"
                  onclick={() => (colorEditId = colorEditId === lab.id ? null : lab.id)}
                >
                  <span class="org-dot"></span>
                  {lab.name}
                </button>
                <button type="button" class="org-chip-x" onclick={() => removeLabel(lab.id)}>×</button>
                {#if colorEditId === lab.id}
                  <div class="org-chip-palette">
                    {#each LABEL_COLORS as c}
                      <button
                        type="button"
                        class="org-swatch"
                        class:on={lab.color.toLowerCase() === c.toLowerCase()}
                        style={`--lab:${c}`}
                        onclick={() => setLabelColor(lab.id, c)}
                      ></button>
                    {/each}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      {:else}
        <p class="muted">Nessuna etichetta.</p>
      {/each}
    </aside>
  </div>
{/if}

<style>
  .org-root {
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
    min-height: 0;
  }

  .org-notice {
    margin: 0 0 0.5rem;
    padding: 0.55rem 0.85rem;
    border-radius: 10px;
    background: var(--success-bg, rgba(61, 184, 154, 0.12));
    color: var(--ink);
    font-size: 0.9rem;
  }

  .org-top {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    align-items: center;
    justify-content: space-between;
  }

  .org-tabs {
    display: inline-flex;
    gap: 0.25rem;
    padding: 0.2rem;
    border-radius: 12px;
    background: color-mix(in srgb, var(--bg2) 80%, transparent);
    border: 1px solid var(--line);
  }

  .org-tab {
    position: relative;
    border: 0;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 0.88rem;
    font-weight: 600;
    padding: 0.45rem 0.9rem;
    border-radius: 10px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }

  .org-tab.active {
    background: var(--panel-bg, var(--bg1));
    color: var(--ink);
  }

  .org-tab-dot {
    width: 0.4rem;
    height: 0.4rem;
    border-radius: 999px;
    background: var(--accent, #5b8def);
  }

  .org-badge {
    font-size: 0.7rem;
    min-width: 1.1rem;
    padding: 0.05rem 0.35rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent, #5b8def) 22%, transparent);
    color: var(--ink);
  }

  .org-top-actions {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    flex-wrap: wrap;
  }

  .org-capture-bar {
    display: grid;
    grid-template-columns: 1fr auto auto auto;
    gap: 0.5rem;
  }

  @media (max-width: 720px) {
    .org-capture-bar {
      grid-template-columns: 1fr;
    }
  }

  .org-filters,
  .org-assign-row {
    display: flex;
    flex-wrap: nowrap;
    gap: 0.35rem;
    overflow-x: auto;
    padding-bottom: 0.15rem;
    scrollbar-width: thin;
  }

  .org-assign-row {
    flex-wrap: wrap;
    align-items: center;
  }

  .org-assign-row.compact {
    max-height: 5.5rem;
    overflow: auto;
  }

  .org-filter {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font: inherit;
    font-size: 0.78rem;
    padding: 0.28rem 0.65rem;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    white-space: nowrap;
  }

  .org-filter.sm {
    padding: 0.18rem 0.5rem;
    font-size: 0.72rem;
  }

  .org-filter.on {
    color: var(--ink);
    border-color: color-mix(in srgb, var(--lab, var(--accent, #5b8def)) 55%, var(--line));
    background: color-mix(in srgb, var(--lab, var(--accent, #5b8def)) 16%, transparent);
  }

  .org-dot {
    width: 0.42rem;
    height: 0.42rem;
    border-radius: 999px;
    background: var(--lab, var(--accent, #5b8def));
  }

  .org-board {
    display: grid;
    gap: 0.75rem;
    align-items: start;
  }

  .org-board.cols-3 {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .org-board.cols-scroll {
    grid-auto-flow: column;
    grid-auto-columns: minmax(240px, 1fr);
    overflow-x: auto;
    padding-bottom: 0.35rem;
  }

  @media (max-width: 980px) {
    .org-board.cols-3 {
      grid-template-columns: 1fr;
    }
  }

  .org-col {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
    min-height: 220px;
    padding: 0.65rem;
    border-radius: 14px;
    border: 1px solid var(--line);
    background: color-mix(in srgb, var(--rail-bg, var(--bg0)) 70%, var(--panel-bg, var(--bg1)));
  }

  .org-col-head {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }

  .org-col-head h3 {
    margin: 0;
    font-size: 0.78rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .org-count {
    display: inline-flex;
    min-width: 1.25rem;
    justify-content: center;
    padding: 0.05rem 0.35rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent, #5b8def) 16%, transparent);
    color: var(--ink);
    font-size: 0.72rem;
  }

  .org-collapse {
    margin-left: auto;
    font-size: 0.75rem;
  }

  .org-col-body {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-height: 2rem;
  }

  .org-col-empty {
    margin: 0.35rem 0;
  }

  .org-col-add {
    margin-top: auto;
    border: 0;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 0.85rem;
    text-align: left;
    padding: 0.45rem 0.35rem;
    cursor: pointer;
    border-radius: 8px;
  }

  .org-col-add:hover {
    color: var(--ink);
    background: color-mix(in srgb, var(--bg2) 50%, transparent);
  }

  .org-card {
    position: relative;
    padding: 0.7rem 0.75rem;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--panel-bg, var(--bg1));
  }

  .org-card.doing {
    border-color: color-mix(in srgb, var(--accent, #5b8def) 40%, var(--line));
  }

  .org-card.prio-high {
    border-left: 3px solid color-mix(in srgb, #e86b8a 80%, transparent);
  }

  .org-card.done {
    opacity: 0.72;
  }

  .org-card.editing {
    display: grid;
    gap: 0.45rem;
  }

  .org-card-top {
    display: flex;
    gap: 0.5rem;
    align-items: flex-start;
  }

  .org-check {
    flex: 0 0 auto;
    width: 1.05rem;
    height: 1.05rem;
    margin-top: 0.15rem;
    border-radius: 999px;
    border: 1.5px solid color-mix(in srgb, var(--ink) 35%, transparent);
    background: transparent;
    cursor: pointer;
  }

  .org-check:hover {
    border-color: var(--accent, #5b8def);
    background: color-mix(in srgb, var(--accent, #5b8def) 18%, transparent);
  }

  .org-card-title {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-weight: 600;
    font-size: 0.9rem;
    line-height: 1.35;
    text-align: left;
    cursor: pointer;
    padding: 0;
  }

  .org-card-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin-top: 0.45rem;
    padding-left: 1.55rem;
  }

  .org-pill {
    font-size: 0.7rem;
    padding: 0.1rem 0.42rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg2) 75%, transparent);
    color: var(--muted);
    border: 1px solid var(--line);
  }

  .org-pill.lab {
    color: var(--ink);
    border-color: color-mix(in srgb, var(--lab, var(--accent)) 45%, var(--line));
    background: color-mix(in srgb, var(--lab, var(--accent)) 12%, transparent);
  }

  .org-pill.hot,
  .org-pill.late {
    color: #e86b8a;
    border-color: color-mix(in srgb, #e86b8a 40%, var(--line));
  }

  .org-menu-wrap {
    position: relative;
    flex: 0 0 auto;
  }

  .org-menu-btn {
    border: 0;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 1rem;
    letter-spacing: 0.05em;
    padding: 0.1rem 0.35rem;
    cursor: pointer;
    border-radius: 6px;
  }

  .org-menu-btn:hover {
    color: var(--ink);
    background: color-mix(in srgb, var(--bg2) 60%, transparent);
  }

  .org-menu {
    position: absolute;
    right: 0;
    top: 100%;
    z-index: 20;
    min-width: 9.5rem;
    display: flex;
    flex-direction: column;
    padding: 0.3rem;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--panel-bg, var(--bg1));
  }

  .org-menu button {
    border: 0;
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-size: 0.85rem;
    text-align: left;
    padding: 0.45rem 0.55rem;
    border-radius: 7px;
    cursor: pointer;
  }

  .org-menu button:hover {
    background: color-mix(in srgb, var(--bg2) 70%, transparent);
  }

  .org-menu button.danger {
    color: #e86b8a;
  }

  .org-edit-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.4rem;
  }

  .org-card-actions {
    display: flex;
    gap: 0.4rem;
  }

  .org-more-done {
    font-size: 0.78rem;
  }

  .panel-like {
    background: var(--panel-bg, var(--bg1));
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 1rem 1.15rem;
  }

  .panel-like h3 {
    margin: 0 0 0.35rem;
    font-size: 1.05rem;
  }

  .panel-desc {
    margin: 0 0 0.85rem;
    color: var(--muted);
    font-size: 0.88rem;
    line-height: 1.45;
  }

  .org-today-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 0.85rem;
  }

  @media (max-width: 1100px) {
    .org-today-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @media (max-width: 640px) {
    .org-today-grid {
      grid-template-columns: 1fr;
    }
  }

  .org-bucket h4 {
    margin: 0 0 0.5rem;
    font-size: 0.78rem;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--muted);
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .org-chip {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
    width: 100%;
    margin: 0 0 0.4rem;
    padding: 0.55rem 0.65rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg2) 55%, transparent);
    color: var(--ink);
    text-align: left;
    cursor: pointer;
    font: inherit;
  }

  .org-chip span:last-child {
    font-size: 0.75rem;
    color: var(--muted);
  }

  .org-chip.overdue {
    border-color: color-mix(in srgb, #e86b8a 45%, var(--line));
  }

  .org-chip.doing {
    border-color: color-mix(in srgb, var(--accent, #5b8def) 40%, var(--line));
  }

  .org-chip.rem {
    cursor: default;
  }

  .org-agenda-form {
    display: grid;
    grid-template-columns: 1fr 1fr auto auto auto auto;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  @media (max-width: 900px) {
    .org-agenda-form {
      grid-template-columns: 1fr;
    }
  }

  .org-agenda-list {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }

  .org-agenda-row {
    display: flex;
    gap: 0.65rem;
    align-items: flex-start;
    justify-content: space-between;
    padding: 0.75rem 0.85rem;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: color-mix(in srgb, var(--bg2) 40%, transparent);
  }

  .org-agenda-main {
    flex: 1;
    min-width: 0;
  }

  .org-agenda-main .org-card-meta {
    padding-left: 0;
  }

  .muted {
    color: var(--muted);
  }

  .tiny {
    font-size: 0.78rem;
  }

  input,
  select {
    font: inherit;
    color: var(--ink);
    background: var(--bg0, #121820);
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 0.55rem 0.7rem;
  }

  button {
    font: inherit;
  }

  .org-drawer-backdrop {
    position: fixed;
    inset: 0;
    z-index: 80;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    justify-content: flex-end;
  }

  .org-drawer {
    width: min(420px, 100%);
    height: 100%;
    overflow: auto;
    background: var(--panel-bg, var(--bg1));
    border-left: 1px solid var(--line);
    padding: 1rem 1.1rem 2rem;
  }

  .org-drawer-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    margin-bottom: 0.35rem;
  }

  .org-drawer-head h3 {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .org-drawer-create {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: 0.4rem;
    margin-bottom: 0.5rem;
  }

  .org-drawer-tools {
    display: flex;
    gap: 0.45rem;
    margin-bottom: 0.85rem;
  }

  .org-drawer-tools input {
    flex: 1;
  }

  .org-swatches,
  .org-chip-palette {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin: 0 0 0.75rem;
  }

  .org-swatch {
    width: 1.05rem;
    height: 1.05rem;
    border-radius: 999px;
    border: 2px solid transparent;
    background: var(--lab);
    cursor: pointer;
    padding: 0;
  }

  .org-swatch.on {
    border-color: var(--ink);
  }

  .org-drawer-group {
    margin-bottom: 0.85rem;
  }

  .org-drawer-group h4 {
    margin: 0 0 0.4rem;
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }

  .org-label-inline {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  .org-chip-lab {
    position: relative;
    display: inline-flex;
    align-items: center;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--lab) 50%, var(--line));
    background: color-mix(in srgb, var(--lab) 14%, transparent);
  }

  .org-chip-lab.editing {
    z-index: 6;
  }

  .org-chip-main {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    border: 0;
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-size: 0.78rem;
    padding: 0.28rem 0.15rem 0.28rem 0.5rem;
    cursor: pointer;
  }

  .org-chip-x {
    border: 0;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 0.95rem;
    line-height: 1;
    padding: 0.2rem 0.45rem 0.2rem 0.1rem;
    cursor: pointer;
  }

  .org-chip-palette {
    position: absolute;
    left: 0;
    top: calc(100% + 0.3rem);
    z-index: 5;
    width: max-content;
    max-width: 220px;
    padding: 0.4rem;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--panel-bg, var(--bg1));
    margin: 0;
  }
</style>
