<script lang="ts">
  import {
    AVATAR_STATE_LABEL,
    normalizeAvatarSkin,
    type AvatarSkinId,
    type AvatarState,
  } from "$lib/avatar";
  import BuddyBot from "$lib/components/avatars/BuddyBot.svelte";
  import Familiar from "$lib/components/avatars/Familiar.svelte";
  import OrbEye from "$lib/components/avatars/OrbEye.svelte";

  interface Props {
    state?: AvatarState;
    size?: "sm" | "md" | "lg" | "float";
    showLabel?: boolean;
    name?: string;
    hideCrest?: boolean;
    /** Avatar skin — default Buddy */
    skin?: AvatarSkinId | string;
    /** Full desktop companion: OS cursor + optional roam */
    live?: boolean;
    roam?: boolean;
    paused?: boolean;
    /** Walk by CSS inside a monitor overlay (do not move the OS window). */
    overlay?: boolean;
    onRoamStep?: (dx: number, dy: number) => void;
    stageCanvas?: HTMLCanvasElement | null;
  }

  let {
    state = "idle",
    size = "md",
    showLabel = false,
    name = "AgentOS",
    hideCrest = false,
    skin = "buddy",
    live = false,
    roam = false,
    paused = false,
    overlay = false,
    onRoamStep,
    stageCanvas = null,
  }: Props = $props();

  const resolved = $derived(normalizeAvatarSkin(skin));
  const label = $derived(AVATAR_STATE_LABEL[state] ?? AVATAR_STATE_LABEL.idle);
  const uid = $derived(`${resolved}-${size}`);
</script>

<figure
  class="avatar2d"
  class:sm={size === "sm"}
  class:md={size === "md"}
  class:lg={size === "lg"}
  class:float={size === "float"}
  class:skin-orb={resolved === "orb"}
  class:skin-familiar={resolved === "familiar"}
  class:skin-buddy={resolved === "buddy"}
  data-state={state}
>
  <div class="stage" aria-hidden="true">
    <div class="halo"></div>
    <div class="pulse-ring"></div>

    {#if resolved === "familiar"}
      <Familiar {state} {uid} {hideCrest} />
    {:else if resolved === "orb"}
      <OrbEye {state} {uid} />
    {:else}
      <BuddyBot
        {state}
        {uid}
        {live}
        {roam}
        {paused}
        {overlay}
        {onRoamStep}
        {stageCanvas}
        framing={size === "float" || size === "lg" ? "full" : "bust"}
      />
    {/if}

    <div class="voice-bars" aria-hidden="true">
      <span></span><span></span><span></span><span></span>
    </div>
  </div>

  {#if showLabel}
    <figcaption>
      <strong>{name}</strong>
      <span class="status">{label}</span>
    </figcaption>
  {/if}
</figure>

<style>
  .avatar2d {
    margin: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.65rem;
    user-select: none;
    background: transparent;
  }

  .stage {
    position: relative;
    display: grid;
    place-items: center;
    background: transparent;
  }

  .sm .stage {
    width: 52px;
    height: 52px;
  }
  .md .stage {
    width: 120px;
    height: 120px;
  }
  .lg .stage {
    width: 180px;
    height: 180px;
  }

  .skin-buddy.sm .stage {
    width: 52px;
    height: 52px;
  }
  .skin-buddy.md .stage {
    width: 120px;
    height: 120px;
  }
  .skin-buddy.lg .stage {
    width: 200px;
    height: 240px;
  }
  .skin-buddy.float .stage {
    width: 100%;
    height: 100%;
  }
  .float .stage {
    width: 100%;
    height: 100%;
  }
  .avatar2d.float {
    width: 100%;
    height: 100%;
  }

  .skin-familiar.sm .stage {
    width: 52px;
    height: 64px;
  }
  .skin-familiar.md .stage {
    width: 120px;
    height: 148px;
  }
  .skin-familiar.lg .stage {
    width: 180px;
    height: 220px;
  }

  .halo {
    position: absolute;
    inset: 6% 8%;
    border-radius: 50%;
    background: radial-gradient(circle, rgba(255, 248, 240, 0.22), transparent 70%);
    z-index: 0;
    filter: blur(2px);
    transition: background 0.4s ease, transform 0.4s ease;
    pointer-events: none;
  }

  .skin-buddy .halo,
  .skin-buddy .pulse-ring,
  .skin-buddy.avatar2d[data-state="thinking"] .halo,
  .skin-buddy.avatar2d[data-state="speaking"] .halo,
  .skin-buddy.avatar2d[data-state="notification"] .halo,
  .skin-buddy.avatar2d[data-state="error"] .halo {
    display: none;
  }

  .skin-familiar .halo {
    background: radial-gradient(circle, rgba(61, 155, 122, 0.22), transparent 70%);
  }

  .pulse-ring {
    position: absolute;
    inset: 16% 18%;
    border-radius: 50%;
    border: 1px solid rgba(255, 248, 240, 0.28);
    z-index: 1;
    opacity: 0;
    pointer-events: none;
  }

  .skin-buddy .pulse-ring {
    border-color: rgba(100, 170, 255, 0.3);
  }

  .skin-familiar .pulse-ring {
    border-color: rgba(196, 163, 90, 0.2);
  }

  .stage > :global(svg),
  .stage > :global(canvas) {
    width: 100%;
    height: 100%;
    position: relative;
    z-index: 2;
  }

  .voice-bars {
    position: absolute;
    bottom: 2%;
    display: none;
    gap: 3px;
    align-items: flex-end;
    height: 14px;
    z-index: 3;
  }

  .voice-bars span {
    width: 3px;
    border-radius: 2px;
    background: rgba(255, 248, 240, 0.85);
    height: 40%;
  }

  .skin-buddy .voice-bars {
    display: none;
  }
  .skin-buddy .voice-bars span {
    background: rgba(90, 170, 255, 0.9);
  }

  figcaption {
    text-align: center;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  figcaption strong {
    font-family: var(--font-display, inherit);
    font-weight: 500;
    font-size: 1.05rem;
    letter-spacing: 0.02em;
  }

  .status {
    color: var(--muted, #8a9a90);
    font-size: 0.8rem;
  }

  .avatar2d[data-state="thinking"] .halo {
    background: radial-gradient(circle, rgba(255, 248, 240, 0.42), transparent 70%);
    animation: think-halo 1.4s ease-in-out infinite;
  }
  .skin-buddy.avatar2d[data-state="thinking"] .halo {
    background: radial-gradient(circle, rgba(120, 180, 255, 0.35), transparent 70%);
  }
  .avatar2d[data-state="thinking"] .pulse-ring {
    opacity: 1;
    animation: ring-out 1.4s ease-out infinite;
  }
  .avatar2d[data-state="thinking"] .status {
    color: var(--accent, #c4a35a);
  }

  .avatar2d[data-state="speaking"] .voice-bars {
    display: flex;
  }
  .skin-buddy.avatar2d[data-state="speaking"] .voice-bars {
    display: none;
  }
  .avatar2d[data-state="speaking"] .voice-bars span:nth-child(1) {
    animation: bar 0.55s ease-in-out infinite;
  }
  .avatar2d[data-state="speaking"] .voice-bars span:nth-child(2) {
    animation: bar 0.55s ease-in-out 0.1s infinite;
  }
  .avatar2d[data-state="speaking"] .voice-bars span:nth-child(3) {
    animation: bar 0.55s ease-in-out 0.2s infinite;
  }
  .avatar2d[data-state="speaking"] .voice-bars span:nth-child(4) {
    animation: bar 0.55s ease-in-out 0.05s infinite;
  }
  .avatar2d[data-state="speaking"] .status {
    color: var(--accent-2, #3d9b7a);
  }

  .avatar2d[data-state="notification"] .halo {
    background: radial-gradient(circle, rgba(126, 200, 255, 0.35), transparent 70%);
    animation: think-halo 0.9s ease-in-out infinite;
  }
  .avatar2d[data-state="notification"] .pulse-ring {
    opacity: 1;
    animation: ring-out 0.9s ease-out infinite;
  }
  .avatar2d[data-state="notification"] .status {
    color: #7ec8ff;
  }

  .avatar2d[data-state="error"] .halo {
    background: radial-gradient(circle, rgba(217, 119, 108, 0.3), transparent 70%);
  }
  .avatar2d[data-state="error"] .status {
    color: var(--danger, #d9776c);
  }

  @keyframes think-halo {
    0%,
    100% {
      transform: scale(1);
      opacity: 0.85;
    }
    50% {
      transform: scale(1.08);
      opacity: 1;
    }
  }
  @keyframes ring-out {
    0% {
      transform: scale(0.85);
      opacity: 0.6;
    }
    100% {
      transform: scale(1.35);
      opacity: 0;
    }
  }
  @keyframes bar {
    0%,
    100% {
      height: 30%;
    }
    50% {
      height: 100%;
    }
  }
</style>
