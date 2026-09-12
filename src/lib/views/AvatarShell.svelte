<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { emit, listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Avatar2D from "$lib/components/Avatar2D.svelte";
  import type { AvatarDesktop } from "$lib/components/avatars/BuddyBot.svelte";
  import type { AvatarState } from "$lib/avatar";
  import { normalizeAvatarSkin } from "$lib/avatar";
  import { avatarLog, runAndLogAvatarProbe } from "$lib/avatarDebug";
  import { applyAvatarShellClass, fetchAvatarDisplayProfile } from "$lib/avatarDisplay";
  import { playReminderChime } from "$lib/notifyChime";

  const SLEEP_DELAY_MS = 20_000;

  let avatarState = $state<AvatarState>("idle");
  let avatarSkin = $state("buddy");
  let avatarRoam = $state(false);
  let chatOpen = $state(false);
  let tip = $state(false);
  let dragging = $state(false);
  let dragError = $state("");
  let pendingAlert = $state("");
  let lastActiveAt = Date.now();
  let sleepTimer: ReturnType<typeof setTimeout> | undefined;

  function wakeUp() {
    lastActiveAt = Date.now();
    if (avatarState === "sleeping") avatarState = "idle";
    clearTimeout(sleepTimer);
    sleepTimer = undefined;
  }

  function scheduleSleep() {
    clearTimeout(sleepTimer);
    const scheduledAt = Date.now();
    lastActiveAt = scheduledAt;
    sleepTimer = setTimeout(() => {
      // Use the scheduledAt snapshot to avoid being restarted by incidental "idle" emissions.
      if (
        avatarState === "idle" &&
        !chatOpen &&
        !dragging &&
        Date.now() - scheduledAt >= SLEEP_DELAY_MS
      ) {
        avatarState = "sleeping";
      }
    }, SLEEP_DELAY_MS);
  }
  let chimeTimer: ReturnType<typeof setInterval> | undefined;
  let skipClick = false;
  let overlayOn = $state(false);
  let spriteLeft = $state(0);
  let spriteTop = $state(0);
  let spriteCssW = $state(168);
  let spriteCssH = $state(252);
  let presentEl: HTMLCanvasElement | undefined = $state();
  let glass = $state(false);

  const overlayWanted = () => false;
  let applyingOverlay = false;
  let lastClickthrough: boolean | null = null;

  function cssScale(d: AvatarDesktop): number {
    const inner = Math.max(1, window.innerWidth);
    return d.win_w / inner;
  }

  function applyDesktopLayout(d: AvatarDesktop) {
    if (window.innerWidth < 8 || window.innerHeight < 8) {
      requestAnimationFrame(() => applyDesktopLayout(d));
      return;
    }
    const s = cssScale(d);
    const sw = d.sprite_w ?? d.win_w;
    const sh = d.sprite_h ?? d.win_h;
    spriteCssW = Math.max(80, sw / s);
    spriteCssH = Math.max(96, sh / s);
    spriteLeft = ((d.sprite_x ?? d.win_x) - d.win_x) / s;
    spriteTop = ((d.sprite_y ?? d.win_y) - d.win_y) / s;
    const maxX = Math.max(0, window.innerWidth - spriteCssW);
    const maxY = Math.max(0, window.innerHeight - spriteCssH);
    spriteLeft = Math.min(maxX, Math.max(0, spriteLeft));
    spriteTop = Math.min(maxY, Math.max(0, spriteTop));
  }

  function clampSpriteCss() {
    const maxX = Math.max(0, window.innerWidth - spriteCssW);
    const maxY = Math.max(0, window.innerHeight - spriteCssH);
    spriteLeft = Math.min(maxX, Math.max(0, spriteLeft));
    spriteTop = Math.min(maxY, Math.max(0, spriteTop));
  }

  async function syncSpritePhysical() {
    if (!overlayOn) return;
    try {
      const d = await invoke<AvatarDesktop>("get_avatar_desktop");
      const s = cssScale(d);
      const x = Math.round(d.win_x + spriteLeft * s);
      const y = Math.round(d.win_y + spriteTop * s);
      await invoke("set_avatar_sprite_pos", { x, y });
    } catch {
      /* ignore */
    }
  }

  async function applyOverlay(want: boolean) {
    if (applyingOverlay) return;
    applyingOverlay = true;
    try {
      const d = await invoke<AvatarDesktop>("set_avatar_overlay", { enabled: want });
      overlayOn = d.overlay === true;
      void avatarLog(
        `overlay enabled=${overlayOn} sprite=${d.sprite_x},${d.sprite_y} ${d.sprite_w}x${d.sprite_h} win=${d.win_w}x${d.win_h}`,
      );
      if (overlayOn) {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            applyDesktopLayout(d);
            void runAndLogAvatarProbe("overlay-on");
          });
        });
      } else {
        lastClickthrough = null;
        void invoke("set_avatar_clickthrough", { ignore: false }).catch(() => {});
        void runAndLogAvatarProbe("overlay-off");
      }
    } catch (err) {
      console.error(err);
      overlayOn = false;
    } finally {
      applyingOverlay = false;
    }
  }

  function onRoamStep(dx: number, dy: number) {
    if (!overlayOn) return;
    const s = Math.max(0.5, window.devicePixelRatio || 1);
    spriteLeft += dx / s;
    spriteTop += dy / s;
    clampSpriteCss();
    void syncSpritePhysical();
  }

  onMount(() => {
    let unsubs: (() => void)[] = [];
    let poll: ReturnType<typeof setInterval> | undefined;

    const loadSkin = async () => {
      try {
        const cfg = await invoke<any>("get_config");
        avatarSkin = normalizeAvatarSkin(cfg?.avatar);
        avatarRoam = cfg?.avatar_roam === true;
      } catch {
        /* keep default */
      }
    };

    (async () => {
      await loadSkin();
      const profile = await fetchAvatarDisplayProfile();
      glass = profile.glass;
      applyAvatarShellClass(glass);
      void invoke("sync_avatar_window").catch(() => {});
      void invoke("set_avatar_overlay", { enabled: false }).catch(() => {});
      void avatarLog(
        `shell mode=${glass ? "glass" : "opaque-window"} os=${profile.os} roam=window-move`,
      );
      scheduleSleep();
      unsubs.push(
        await listen<string>("avatar-state", (ev) => {
          if (pendingAlert && ev.payload !== "notification") return;
          const next = ev.payload as AvatarState;
          const prev = avatarState;
          avatarState = next;
          if (next === "idle") {
            // Start counting only when we *enter* idle.
            if (prev !== "idle") scheduleSleep();
          } else {
            wakeUp();
          }
        }),
      );
      unsubs.push(
        await listen<any>("reminder-due", (ev) => {
          startAlert(String(ev.payload?.message ?? "Hai un promemoria"));
        }),
      );
      unsubs.push(
        await listen("reminder-acked", () => {
          clearAlert(false);
        }),
      );
      unsubs.push(
        await listen<any>("avatar-debug-probe", () => {
          void runAndLogAvatarProbe("debug-tab");
        }),
      );
      unsubs.push(
        await listen<any>("config-updated", (ev) => {
          if (ev.payload?.avatar != null) {
            avatarSkin = normalizeAvatarSkin(ev.payload.avatar);
          }
          if (ev.payload?.avatar_roam != null) {
            avatarRoam = ev.payload.avatar_roam === true;
          }
          if (ev.payload?.avatar == null && ev.payload?.avatar_roam == null) {
            void loadSkin();
          }
        }),
      );

    })();

    poll = setInterval(() => {
      if (!overlayOn || dragging) {
        if (!overlayOn && lastClickthrough) {
          lastClickthrough = false;
          void invoke("set_avatar_clickthrough", { ignore: false }).catch(() => {});
        }
        return;
      }
      if (pendingAlert) {
        if (lastClickthrough) {
          lastClickthrough = false;
          void invoke("set_avatar_clickthrough", { ignore: false }).catch(() => {});
        }
        return;
      }
      void invoke<AvatarDesktop>("get_avatar_desktop")
        .then((d) => {
          const pad = 18;
          const x = d.sprite_x ?? d.win_x;
          const y = d.sprite_y ?? d.win_y;
          const w = d.sprite_w ?? d.win_w;
          const h = d.sprite_h ?? d.win_h;
          const over =
            d.cursor_x >= x - pad &&
            d.cursor_x <= x + w + pad &&
            d.cursor_y >= y - pad &&
            d.cursor_y <= y + h + pad;
          tip = over;
          const ignore = !over;
          if (ignore !== lastClickthrough) {
            lastClickthrough = ignore;
            void invoke("set_avatar_clickthrough", { ignore }).catch(() => {});
          }
        })
        .catch(() => {});
    }, 50);

    const onFocus = () => {
      void loadSkin();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      unsubs.forEach((u) => u());
      window.removeEventListener("focus", onFocus);
      if (poll) clearInterval(poll);
      if (chimeTimer) clearInterval(chimeTimer);
      clearTimeout(sleepTimer);
      void invoke("set_avatar_clickthrough", { ignore: false }).catch(() => {});
    };
  });

  $effect(() => {
    const want = overlayWanted();
    if (want === overlayOn) return;
    void applyOverlay(want);
  });

  function startAlert(message: string) {
    pendingAlert = message.trim() || "Hai un promemoria";
    avatarState = "notification";
    wakeUp(); // cancel any pending sleep while we show an alert
    playReminderChime();
    if (chimeTimer) clearInterval(chimeTimer);
    let extra = 0;
    chimeTimer = setInterval(() => {
      extra += 1;
      if (extra >= 3 || !pendingAlert) {
        if (chimeTimer) clearInterval(chimeTimer);
        chimeTimer = undefined;
        return;
      }
      playReminderChime();
    }, 14_000);
  }

  function clearAlert(broadcast: boolean) {
    pendingAlert = "";
    if (chimeTimer) {
      clearInterval(chimeTimer);
      chimeTimer = undefined;
    }
    if (avatarState === "notification") {
      avatarState = "idle";
      if (!chatOpen && !dragging) scheduleSleep();
    }
    if (broadcast) void emit("reminder-acked");
  }

  function ackAlert(e?: Event) {
    e?.preventDefault();
    e?.stopPropagation();
    clearAlert(true);
  }

  async function toggleChat() {
    if (dragging) return;
    wakeUp();
    try {
      chatOpen = await invoke<boolean>("toggle_chat_panel");
    } catch (err) {
      console.error(err);
    }
    if (!chatOpen && avatarState === "idle" && !pendingAlert) scheduleSleep();
  }

  async function beginDragFrom(clientX: number, clientY: number) {
    if (dragging) return;
    skipClick = true;
    tip = false;
    dragError = "";
    dragging = true;
    wakeUp(); // ensure we don't fall asleep during drag
    lastClickthrough = false;
    void invoke("set_avatar_clickthrough", { ignore: false }).catch(() => {});

    if (overlayOn) {
      const grabX = clientX - spriteLeft;
      const grabY = clientY - spriteTop;
      const move = (ev: PointerEvent) => {
        spriteLeft = ev.clientX - grabX;
        spriteTop = ev.clientY - grabY;
        clampSpriteCss();
      };
      const up = () => {
        dragging = false;
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        void syncSpritePhysical();
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      return;
    }

    try {
      await invoke("begin_avatar_drag");
    } catch (err) {
      dragError = String(err);
      console.error(err);
      dragging = false;
      return;
    }

    const clear = () => {
      dragging = false;
      window.removeEventListener("pointerup", clear);
      window.removeEventListener("mouseup", clear);
    };
    window.addEventListener("pointerup", clear, { once: true });
    window.addEventListener("mouseup", clear, { once: true });
    setTimeout(() => {
      if (dragging) dragging = false;
    }, 30_000);
  }

  function onBodyPointerDown(e: PointerEvent) {
    if (e.button !== 0 || dragging) return;
    skipClick = false;
    const startX = e.clientX;
    const startY = e.clientY;
    let started = false;
    let lastX = startX;
    let lastY = startY;

    const begin = () => {
      if (started || dragging) return;
      started = true;
      void beginDragFrom(lastX, lastY);
    };

    const onMove = (ev: PointerEvent) => {
      lastX = ev.clientX;
      lastY = ev.clientY;
      if (Math.hypot(ev.clientX - startX, ev.clientY - startY) > 10) {
        window.clearTimeout(holdTimer);
        begin();
      }
    };
    const onUp = () => {
      window.clearTimeout(holdTimer);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      if (!started && !skipClick) void toggleChat();
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    const holdTimer = window.setTimeout(begin, 300);
  }

  async function openConsole(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    tip = false;
    try {
      await invoke("open_console");
    } catch (err) {
      console.error(err);
    }
  }
</script>

<div
  class="float-root"
  class:dragging
  class:overlay={overlayOn}
  class:glass
  class:alerting={!!pendingAlert}
  role="application"
  aria-label="Buddy, assistente sul desktop"
  oncontextmenu={openConsole}
  onmouseenter={() => (tip = true)}
  onmouseleave={() => (tip = false)}
>
  <div class="present-wrap" aria-hidden="true">
    <canvas bind:this={presentEl}></canvas>
  </div>

  {#if avatarSkin !== "buddy" || (avatarState !== "idle" && avatarState !== "sleeping") || pendingAlert}
    <div class="glow" class:buddy-glow={avatarSkin === "buddy"} data-state={pendingAlert ? "notification" : avatarState}></div>
  {/if}

  <div
    class="companion"
    style={overlayOn
      ? `left:${spriteLeft}px;top:${spriteTop}px;width:${spriteCssW}px;height:${spriteCssH}px`
      : undefined}
  >
    <button
      type="button"
      class="body-hit"
      class:body-hit-3d={avatarSkin === "buddy"}
      aria-label="Apri o chiudi chat. Tieni premuto per spostare."
      title="Clic: chat · Tieni premuto: sposta"
      onpointerdown={onBodyPointerDown}
      onkeydown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          if (pendingAlert) ackAlert();
          else void toggleChat();
        }
      }}
    >
      {#key avatarSkin}
        <Avatar2D
          state={pendingAlert ? "notification" : avatarState}
          size="float"
          showLabel={false}
          hideCrest={true}
          skin={avatarSkin}
          live={true}
          roam={avatarRoam}
          paused={dragging || chatOpen || !!pendingAlert}
          overlay={overlayOn}
          {onRoamStep}
          stageCanvas={overlayOn ? presentEl ?? null : null}
        />
      {/key}
    </button>

    {#if pendingAlert}
      <div
        class="alert-card"
        role="alertdialog"
        tabindex="-1"
        aria-live="assertive"
        aria-label="Promemoria"
        onpointerdown={(e) => e.stopPropagation()}
      >
        <p class="alert-msg">{pendingAlert}</p>
        <button type="button" class="alert-ok" onpointerdown={ackAlert}>Ok, ci penso</button>
      </div>
    {/if}

    {#if tip && !dragging && !pendingAlert}
      <p class="hint">Tieni premuto: sposta · Clic: chat · Dx: console</p>
    {/if}
    {#if dragError}
      <p class="hint err">{dragError}</p>
    {/if}
  </div>
</div>

<style>
  :global(html.shell-avatar),
  :global(html.shell-avatar body),
  :global(html.shell-avatar #boot) {
    background: #0e1512 !important;
    overflow: hidden;
  }

  :global(html.shell-avatar.shell-avatar-glass),
  :global(html.shell-avatar.shell-avatar-glass body),
  :global(html.shell-avatar.shell-avatar-glass #boot) {
    background: transparent !important;
  }

  .float-root {
    width: 100vw;
    height: 100vh;
    display: grid;
    place-items: center;
    position: relative;
    -webkit-user-select: none;
    user-select: none;
    background: #0e1512;
  }

  .float-root.dragging {
    cursor: grabbing;
  }

  .float-root.alerting .glow[data-state="notification"] {
    animation: alert-glow 1.1s ease-in-out infinite;
  }
  .float-root.alerting .glow.buddy-glow[data-state="notification"] {
    animation: alert-glow-buddy 1.1s ease-in-out infinite;
  }

  .float-root.overlay {
    display: block;
    pointer-events: none;
  }

  .float-root.glass {
    background: transparent;
  }

  .companion {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 100%;
    height: 100%;
  }

  .overlay .companion {
    position: absolute;
    pointer-events: auto;
    z-index: 2;
  }

  .present-wrap {
    display: none;
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 1;
  }
  .overlay .present-wrap {
    display: block;
  }
  .present-wrap canvas {
    width: 100%;
    height: 100%;
    display: block;
  }

  .body-hit {
    display: block;
    padding: 0;
    margin: 0;
    border: none;
    background: transparent;
    cursor: grab;
    border-radius: 50% 50% 45% 45%;
    width: 100%;
    height: 100%;
    touch-action: none;
  }

  .dragging .body-hit {
    cursor: grabbing;
  }

  .body-hit-3d {
    border-radius: 24px;
  }

  .body-hit:focus-visible {
    outline: 2px solid rgba(196, 163, 90, 0.55);
    outline-offset: 4px;
  }

  .alert-card {
    position: absolute;
    left: 50%;
    bottom: 8%;
    transform: translateX(-50%);
    z-index: 8;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.4rem;
    width: min(92%, 210px);
    padding: 0.55rem 0.65rem 0.5rem;
    border-radius: 14px;
    background: rgba(14, 21, 18, 0.92);
    border: 1px solid rgba(126, 200, 255, 0.45);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    pointer-events: auto;
  }

  .alert-msg {
    margin: 0;
    font-size: 0.72rem;
    line-height: 1.35;
    text-align: center;
    color: #e8f4ff;
    max-height: 4.2em;
    overflow: hidden;
  }

  .alert-ok {
    border: none;
    border-radius: 999px;
    padding: 0.28rem 0.85rem;
    font-size: 0.7rem;
    font-weight: 600;
    cursor: pointer;
    color: #0e1512;
    background: #7ec8ff;
  }

  .alert-ok:hover {
    filter: brightness(1.08);
  }

  .glow {
    position: absolute;
    width: 140px;
    height: 140px;
    border-radius: 50%;
    background: radial-gradient(circle, rgba(120, 180, 255, 0.22), transparent 72%);
    pointer-events: none;
    transition: background 0.35s ease;
    filter: blur(1px);
  }

  .glow.buddy-glow {
    width: 100px;
    height: 100px;
    top: 8%;
    left: 50%;
    transform: translateX(-50%);
    opacity: 0.85;
  }

  .glow[data-state="thinking"] {
    background: radial-gradient(circle, rgba(255, 248, 240, 0.55), transparent 70%);
  }
  .glow[data-state="speaking"] {
    background: radial-gradient(circle, rgba(255, 248, 240, 0.5), transparent 70%);
  }
  .glow[data-state="notification"] {
    background: radial-gradient(circle, rgba(126, 200, 255, 0.4), transparent 70%);
  }
  .glow[data-state="error"] {
    background: radial-gradient(circle, rgba(217, 119, 108, 0.35), transparent 70%);
  }

  .hint {
    position: absolute;
    bottom: 6px;
    left: 50%;
    transform: translateX(-50%);
    margin: 0;
    padding: 0.2rem 0.5rem;
    border-radius: 999px;
    font-size: 0.65rem;
    color: var(--ink);
    background: rgba(14, 21, 18, 0.82);
    border: 1px solid var(--line);
    white-space: nowrap;
    pointer-events: none;
    max-width: 90vw;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .hint.err {
    color: #f0b4ad;
    border-color: rgba(217, 119, 108, 0.5);
    bottom: 28px;
  }

  @keyframes alert-glow {
    0%,
    100% {
      opacity: 0.55;
      transform: scale(1);
    }
    50% {
      opacity: 1;
      transform: scale(1.22);
    }
  }
  @keyframes alert-glow-buddy {
    0%,
    100% {
      opacity: 0.55;
      transform: translateX(-50%) scale(1);
    }
    50% {
      opacity: 1;
      transform: translateX(-50%) scale(1.22);
    }
  }
</style>
