<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AvatarState } from "$lib/avatar";
  import type { Buddy3DFraming, Buddy3DHandle } from "./buddy3d";
  import { fetchAvatarDisplayProfile } from "$lib/avatarDisplay";

  export interface AvatarDesktop {
    cursor_x: number;
    cursor_y: number;
    win_x: number;
    win_y: number;
    win_w: number;
    win_h: number;
    mon_x: number;
    mon_y: number;
    mon_w: number;
    mon_h: number;
    overlay?: boolean;
    sprite_x?: number;
    sprite_y?: number;
    sprite_w?: number;
    sprite_h?: number;
  }

  interface Props {
    state?: AvatarState;
    uid?: string;
    live?: boolean;
    roam?: boolean;
    paused?: boolean;
    overlay?: boolean;
    onRoamStep?: (dx: number, dy: number) => void;
    stageCanvas?: HTMLCanvasElement | null;
    framing?: Buddy3DFraming;
  }

  let {
    state: avatarState = "idle",
    uid: _uid = "buddy",
    live = false,
    roam = false,
    paused = false,
    overlay = false,
    onRoamStep,
    stageCanvas = null,
    framing = "bust",
  }: Props = $props();

  let canvasEl: HTMLCanvasElement | undefined = $state();
  let handle: Buddy3DHandle | null = $state(null);
  let failed = $state(false);
  let glass = $state(false);
  const flags = {
    roam: false,
    paused: false,
    overlay: false,
    dirX: 1,
    dirY: 0,
    onRoamStep: undefined as ((dx: number, dy: number) => void) | undefined,
  };

  $effect(() => {
    flags.roam = roam;
    flags.paused = paused;
    flags.overlay = overlay;
    flags.onRoamStep = onRoamStep;
    if (!handle) return;
    if (!roam || paused) handle.setGait(false, 0, 0);
    else handle.setGait(true, flags.dirX, flags.dirY);
  });

  onMount(() => {
    let cancelled = false;
    let poll: ReturnType<typeof setInterval> | undefined;
    let dirTimer: ReturnType<typeof setInterval> | undefined;

    const pickDir = () => {
      const a = Math.random() * Math.PI * 2;
      flags.dirX = Math.cos(a);
      flags.dirY = Math.sin(a) * 0.35;
    };
    pickDir();

    (async () => {
      if (!canvasEl) return;
      try {
        const { mountBuddy3D } = await import("./buddy3d");
        if (cancelled || !canvasEl) return;
        const profile = await fetchAvatarDisplayProfile();
        if (cancelled || !canvasEl) return;
        glass = live && profile.glass;
        handle = await mountBuddy3D(canvasEl, {
          state: avatarState,
          live,
          framing,
          glass,
          presentCanvas: overlay ? stageCanvas : null,
          onStep: (dirX, dirY) => {
            if (!flags.roam || flags.paused) return;
            const dx = Math.round(dirX * 11);
            const dy = Math.round(dirY * 11);
            if (dx === 0 && dy === 0) return;
            void invoke("move_avatar_by", { dx, dy }).catch(() => {
              /* ignore */
            });
          },
        });
        if (cancelled) {
          handle.dispose();
          handle = null;
          return;
        }
      } catch (e) {
        console.error("Buddy3D init failed", e);
        const msg = e instanceof Error ? e.message : String(e);
        void import("$lib/avatarDebug").then(({ avatarLog }) =>
          avatarLog(`3d init failed: ${msg}`, "warn"),
        );
        failed = true;
        return;
      }

      if (!live || cancelled) return;

      poll = setInterval(() => {
        if (cancelled || !handle) return;
        void invoke<AvatarDesktop>("get_avatar_desktop")
          .then((d) => {
            const ox = d.overlay === true;
            handle?.setPointer(
              d.cursor_x,
              d.cursor_y,
              ox ? (d.sprite_x ?? d.win_x) : d.win_x,
              ox ? (d.sprite_y ?? d.win_y) : d.win_y,
              ox ? (d.sprite_w ?? d.win_w) : d.win_w,
              ox ? (d.sprite_h ?? d.win_h) : d.win_h,
            );
          })
          .catch(() => {
            /* webview-only fallback already in 3D */
          });
      }, 50);

      dirTimer = setInterval(() => {
        if (cancelled || !handle) return;
        if (!flags.roam || flags.paused) {
          handle.setGait(false, 0, 0);
          return;
        }
        if (Math.random() < 0.28) pickDir();
        handle.setGait(true, flags.dirX, flags.dirY);
      }, 1800);
    })();

    return () => {
      cancelled = true;
      if (poll) clearInterval(poll);
      if (dirTimer) clearInterval(dirTimer);
      handle?.dispose();
      handle = null;
    };
  });

  $effect(() => {
    handle?.setState(avatarState);
  });
</script>

{#if failed}
  <div class="fallback" aria-hidden="true">Buddy</div>
{:else}
  <div class="buddy3d-wrap" class:using-stage={overlay} class:glass>
    <canvas
      bind:this={canvasEl}
      class="buddy3d"
      data-state={avatarState}
      aria-label="Avatar Buddy 3D"
    ></canvas>
  </div>
{/if}

<style>
  .buddy3d-wrap {
    position: relative;
    width: 100%;
    height: 100%;
    background: transparent;
  }
  .buddy3d-wrap.using-stage :global(.buddy3d-present) {
    display: none !important;
  }
  .buddy3d-wrap.glass .buddy3d {
    opacity: 1;
  }
  .buddy3d {
    width: 100%;
    height: 100%;
    display: block;
    touch-action: none;
    outline: none;
    opacity: 0;
    pointer-events: none;
    background: transparent;
  }
  .fallback {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    color: #8ab;
    font-size: 0.75rem;
  }
</style>
