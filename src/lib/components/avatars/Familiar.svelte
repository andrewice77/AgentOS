<script lang="ts">
  import type { AvatarState } from "$lib/avatar";

  interface Props {
    state?: AvatarState;
    uid?: string;
    hideCrest?: boolean;
  }

  let { state = "idle", uid = "fam", hideCrest = false }: Props = $props();

  const bodyGrad = $derived(`bodyGrad-${uid}`);
  const coreGrad = $derived(`coreGrad-${uid}`);
  const glowGrad = $derived(`glowGrad-${uid}`);
  const softGlow = $derived(`softGlow-${uid}`);
</script>

<svg class="familiar" data-state={state} viewBox="0 0 160 200" role="img" aria-label="Avatar Familiar">
  <defs>
    <linearGradient id={bodyGrad} x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#2a4a3c" />
      <stop offset="45%" stop-color="#1a2e26" />
      <stop offset="100%" stop-color="#0f1a15" />
    </linearGradient>
    <linearGradient id={coreGrad} x1="50%" y1="0%" x2="50%" y2="100%">
      <stop offset="0%" stop-color="#c4a35a" stop-opacity="0.95" />
      <stop offset="100%" stop-color="#3d9b7a" stop-opacity="0.85" />
    </linearGradient>
    <radialGradient id={glowGrad} cx="50%" cy="40%" r="55%">
      <stop offset="0%" stop-color="#3d9b7a" stop-opacity="0.55" />
      <stop offset="70%" stop-color="#c4a35a" stop-opacity="0.12" />
      <stop offset="100%" stop-color="#0e1512" stop-opacity="0" />
    </radialGradient>
    <filter id={softGlow} x="-40%" y="-40%" width="180%" height="180%">
      <feGaussianBlur stdDeviation="3.5" result="b" />
      <feMerge>
        <feMergeNode in="b" />
        <feMergeNode in="SourceGraphic" />
      </feMerge>
    </filter>
  </defs>

  <ellipse class="ambient" cx="80" cy="108" rx="58" ry="72" fill={`url(#${glowGrad})`} />
  <path class="lobe left" d="M48 95 C28 88 22 118 36 138 C42 128 52 118 58 112 Z" fill="#1c2a23" opacity="0.85" />
  <path class="lobe right" d="M112 95 C132 88 138 118 124 138 C118 128 108 118 102 112 Z" fill="#1c2a23" opacity="0.85" />
  <path
    class="body"
    d="M80 28 C108 28 128 52 128 86 C128 118 114 148 96 168 C90 176 84 182 80 186 C76 182 70 176 64 168 C46 148 32 118 32 86 C32 52 52 28 80 28 Z"
    fill={`url(#${bodyGrad})`}
    stroke="rgba(196,163,90,0.35)"
    stroke-width="1.5"
  />
  <ellipse class="core" cx="80" cy="108" rx="22" ry="30" fill={`url(#${coreGrad})`} filter={`url(#${softGlow})`} />
  <ellipse class="core-shine" cx="74" cy="96" rx="8" ry="12" fill="rgba(231,240,234,0.28)" />
  <ellipse class="face" cx="80" cy="72" rx="28" ry="24" fill="#15201b" opacity="0.55" />
  <g class="eyes">
    <ellipse class="eye left" cx="68" cy="70" rx="5.5" ry="6.5" fill="#e7f0ea" />
    <ellipse class="eye right" cx="92" cy="70" rx="5.5" ry="6.5" fill="#e7f0ea" />
    <circle class="pupil left" cx="69" cy="71" r="2.4" fill="#0e1512" />
    <circle class="pupil right" cx="93" cy="71" r="2.4" fill="#0e1512" />
    <circle class="spark left" cx="67" cy="68.5" r="1.1" fill="#fff" opacity="0.85" />
    <circle class="spark right" cx="91" cy="68.5" r="1.1" fill="#fff" opacity="0.85" />
  </g>
  <path class="mouth" d="M72 86 Q80 90 88 86" fill="none" stroke="#9bb0a4" stroke-width="2" stroke-linecap="round" />
  {#if !hideCrest}
    <path class="crest" d="M80 28 C78 18 74 12 80 6 C86 12 82 18 80 28" fill="#c4a35a" opacity="0.75" />
    <circle class="crest-dot" cx="80" cy="8" r="3.5" fill="#3d9b7a" filter={`url(#${softGlow})`} />
  {/if}
</svg>

<style>
  .familiar {
    width: 100%;
    height: 100%;
    overflow: visible;
    display: block;
  }

  .core,
  .mouth,
  .crest-dot,
  .eye {
    transform-box: fill-box;
    transform-origin: center;
  }

  .familiar[data-state="idle"] {
    animation: breath 4.2s ease-in-out infinite;
  }
  .familiar[data-state="idle"] .crest-dot {
    animation: twinkle 3.2s ease-in-out infinite;
  }
  .familiar[data-state="idle"] .eye {
    animation: blink 5.5s infinite;
  }

  .familiar[data-state="thinking"] .core {
    animation: think-core 1.1s ease-in-out infinite;
  }
  .familiar[data-state="thinking"] .crest-dot {
    animation: spin-soft 2.4s linear infinite;
  }

  .familiar[data-state="speaking"] .mouth {
    stroke: var(--accent-2, #3d9b7a);
    animation: talk 0.45s ease-in-out infinite;
  }
  .familiar[data-state="speaking"] .core {
    animation: speak-glow 0.9s ease-in-out infinite;
  }

  .familiar[data-state="notification"] .crest-dot {
    fill: #7ec8ff;
    animation: ping 0.85s ease-out infinite;
  }
  .familiar[data-state="notification"] {
    animation: hop-alert 0.65s ease-in-out infinite;
  }

  .familiar[data-state="error"] .core {
    opacity: 0.35;
  }
  .familiar[data-state="error"] .crest-dot {
    fill: var(--danger, #d9776c);
  }
  .familiar[data-state="error"] .mouth {
    stroke: var(--danger, #d9776c);
    d: path("M72 90 Q80 84 88 90");
  }

  @keyframes breath {
    0%,
    100% {
      transform: translateY(0) scale(1);
    }
    50% {
      transform: translateY(-3px) scale(1.015);
    }
  }
  @keyframes twinkle {
    0%,
    100% {
      opacity: 0.7;
      transform: scale(1);
    }
    50% {
      opacity: 1;
      transform: scale(1.25);
    }
  }
  @keyframes blink {
    0%,
    92%,
    100% {
      transform: scaleY(1);
    }
    94%,
    96% {
      transform: scaleY(0.08);
    }
  }
  @keyframes think-core {
    0%,
    100% {
      opacity: 0.75;
      transform: scale(0.95);
    }
    50% {
      opacity: 1;
      transform: scale(1.08);
    }
  }
  @keyframes spin-soft {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes talk {
    0%,
    100% {
      transform: translateY(0);
      stroke-width: 2;
    }
    50% {
      transform: translateY(1.5px);
      stroke-width: 2.6;
    }
  }
  @keyframes speak-glow {
    0%,
    100% {
      opacity: 0.85;
      transform: scale(1);
    }
    50% {
      opacity: 1;
      transform: scale(1.06);
    }
  }
  @keyframes ping {
    0% {
      transform: scale(1);
      opacity: 1;
    }
    100% {
      transform: scale(1.8);
      opacity: 0.2;
    }
  }
  @keyframes nod {
    0%,
    100% {
      transform: rotate(0deg);
    }
    30% {
      transform: rotate(-4deg);
    }
    60% {
      transform: rotate(3deg);
    }
  }
  @keyframes hop-alert {
    0%,
    100% {
      transform: translateY(0);
    }
    50% {
      transform: translateY(-10px);
    }
  }
</style>
