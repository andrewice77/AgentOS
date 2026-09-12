<script lang="ts">
  import type { AvatarState } from "$lib/avatar";

  interface Props {
    state?: AvatarState;
    uid?: string;
  }

  let { state = "idle", uid = "orb" }: Props = $props();

  const shell = $derived(`shell-${uid}`);
  const well = $derived(`well-${uid}`);
  const iris = $derived(`iris-${uid}`);
  const bloom = $derived(`bloom-${uid}`);
  const glow = $derived(`glow-${uid}`);
  const soft = $derived(`soft-${uid}`);
</script>

<svg
  class="orb"
  data-state={state}
  viewBox="0 0 200 200"
  role="img"
  aria-label="Avatar Lumina"
>
  <defs>
    <radialGradient id={shell} cx="38%" cy="28%" r="72%">
      <stop offset="0%" stop-color="#fff8f0" />
      <stop offset="42%" stop-color="#f0e4d4" />
      <stop offset="78%" stop-color="#d9c4ae" />
      <stop offset="100%" stop-color="#b89a7e" />
    </radialGradient>
    <radialGradient id={well} cx="50%" cy="45%" r="55%">
      <stop offset="0%" stop-color="#1a1420" />
      <stop offset="55%" stop-color="#0c0a10" />
      <stop offset="100%" stop-color="#050406" />
    </radialGradient>
    <radialGradient id={iris} cx="42%" cy="38%" r="60%">
      <stop offset="0%" stop-color="#fff0d8" />
      <stop offset="35%" stop-color="#f0c090" />
      <stop offset="70%" stop-color="#d4a06a" />
      <stop offset="100%" stop-color="#a07850" stop-opacity="0.2" />
    </radialGradient>
    <radialGradient id={bloom} cx="50%" cy="50%" r="50%">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.65" />
      <stop offset="35%" stop-color="#fff8f0" stop-opacity="0.35" />
      <stop offset="100%" stop-color="#fff8f0" stop-opacity="0" />
    </radialGradient>
    <filter id={glow} x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="2.2" result="b" />
      <feMerge>
        <feMergeNode in="b" />
        <feMergeNode in="SourceGraphic" />
      </feMerge>
    </filter>
    <filter id={soft} x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="1.2" />
    </filter>
  </defs>

  <circle class="bloom" cx="100" cy="100" r="78" fill={`url(#${bloom})`} />
  <circle class="ring r1" cx="100" cy="100" r="62" fill="none" stroke="rgba(255,248,240,0.4)" stroke-width="1.5" />
  <circle class="ring r2" cx="100" cy="100" r="62" fill="none" stroke="rgba(240,228,212,0.3)" stroke-width="1" />

  <circle class="shell" cx="100" cy="100" r="58" fill={`url(#${shell})`} filter={`url(#${soft})`} />
  <circle class="shell-rim" cx="100" cy="100" r="58" fill="none" stroke="rgba(255,255,255,0.45)" stroke-width="1.25" />
  <circle class="shell-inner" cx="100" cy="100" r="49" fill="none" stroke="rgba(60,40,35,0.18)" stroke-width="2" />

  <circle class="well" cx="100" cy="100" r="38" fill={`url(#${well})`} />

  <g class="gaze">
    <circle class="iris" cx="100" cy="100" r="16" fill={`url(#${iris})`} filter={`url(#${glow})`} />
    <circle class="pupil" cx="100" cy="100" r="5" fill="#2a2018" />
    <circle class="pupil-soft" cx="100" cy="100" r="8" fill="#f0c090" opacity="0.2" />
    <ellipse class="spark big" cx="93" cy="91" rx="5" ry="6.5" fill="#fff" opacity="0.92" />
    <circle class="spark tiny" cx="107" cy="107" r="2" fill="#fff8f0" opacity="0.8" />
  </g>

  <path
    class="smile"
    d="M86 128 Q100 136 114 128"
    fill="none"
    stroke="rgba(255,248,240,0.4)"
    stroke-width="2"
    stroke-linecap="round"
  />

  <g class="motes" opacity="0.65">
    <circle class="mote m1" cx="58" cy="70" r="2" fill="#fff8f0" />
    <circle class="mote m2" cx="148" cy="118" r="1.6" fill="#f5ebe0" />
    <circle class="mote m3" cx="130" cy="58" r="1.3" fill="#ffffff" />
  </g>
</svg>

<style>
  .orb {
    width: 100%;
    height: 100%;
    overflow: visible;
    display: block;
  }

  .bloom,
  .shell,
  .gaze,
  .iris,
  .smile,
  .ring,
  .mote {
    transform-box: fill-box;
    transform-origin: center;
  }

  .gaze {
    transform-origin: 100px 100px;
  }

  .bloom,
  .ring {
    transform-origin: 100px 100px;
  }

  .ring {
    opacity: 0;
  }

  .orb[data-state="idle"] {
    animation: floaty 4.8s ease-in-out infinite;
  }
  .orb[data-state="idle"] .bloom {
    animation: bloom-soft 4.8s ease-in-out infinite;
  }
  .orb[data-state="idle"] .gaze {
    animation: look-around 9s ease-in-out infinite;
  }
  .orb[data-state="idle"] .iris {
    animation: blink-iris 5.8s ease-in-out infinite;
  }
  .orb[data-state="idle"] .smile {
    opacity: 0.55;
  }
  .orb[data-state="idle"] .m1 {
    animation: mote 3.6s ease-in-out infinite;
  }
  .orb[data-state="idle"] .m2 {
    animation: mote 4.2s ease-in-out 0.4s infinite;
  }
  .orb[data-state="idle"] .m3 {
    animation: mote 3.2s ease-in-out 0.8s infinite;
  }

  .orb[data-state="thinking"] .bloom {
    animation: bloom-think 1.2s ease-in-out infinite;
  }
  .orb[data-state="thinking"] .iris {
    animation: think-pulse 1.1s ease-in-out infinite;
  }
  .orb[data-state="thinking"] .r1 {
    opacity: 1;
    animation: ring-expand 1.5s ease-out infinite;
  }
  .orb[data-state="thinking"] .r2 {
    opacity: 1;
    animation: ring-expand 1.5s ease-out 0.45s infinite;
  }
  .orb[data-state="thinking"] .gaze {
    animation: curious 2.4s ease-in-out infinite;
  }
  .orb[data-state="thinking"] .smile {
    opacity: 0.25;
  }

  .orb[data-state="speaking"] .iris {
    animation: speak-iris 0.7s ease-in-out infinite;
  }
  .orb[data-state="speaking"] .r1 {
    opacity: 1;
    animation: ring-expand 0.9s ease-out infinite;
  }
  .orb[data-state="speaking"] .r2 {
    opacity: 1;
    animation: ring-expand 0.9s ease-out 0.3s infinite;
  }
  .orb[data-state="speaking"] .smile {
    stroke: rgba(255, 248, 240, 0.75);
    animation: smile-talk 0.55s ease-in-out infinite;
  }
  .orb[data-state="speaking"] .bloom {
    animation: bloom-speak 0.9s ease-in-out infinite;
  }

  .orb[data-state="notification"] {
    animation: hop 0.7s ease-in-out infinite;
  }
  .orb[data-state="notification"] .iris {
    animation: notify-flash 0.8s ease-in-out infinite;
  }
  .orb[data-state="notification"] .r1 {
    opacity: 1;
    stroke: rgba(255, 248, 240, 0.5);
    animation: ring-expand 0.85s ease-out infinite;
  }
  .orb[data-state="notification"] .smile {
    opacity: 0.85;
    stroke: rgba(255, 248, 240, 0.55);
  }

  .orb[data-state="error"] .iris {
    opacity: 0.45;
    animation: none;
  }
  .orb[data-state="error"] .bloom {
    opacity: 0.35;
  }
  .orb[data-state="error"] .smile {
    d: path("M86 134 Q100 126 114 134");
    stroke: rgba(217, 119, 108, 0.55);
    opacity: 0.8;
  }
  .orb[data-state="error"] .gaze {
    animation: none;
    transform: translateY(2px);
  }

  @keyframes floaty {
    0%,
    100% {
      transform: translateY(0) scale(1);
    }
    50% {
      transform: translateY(-4px) scale(1.02);
    }
  }
  @keyframes bloom-soft {
    0%,
    100% {
      opacity: 0.75;
      transform: scale(1);
    }
    50% {
      opacity: 1;
      transform: scale(1.06);
    }
  }
  @keyframes bloom-think {
    0%,
    100% {
      opacity: 0.7;
      transform: scale(1);
    }
    50% {
      opacity: 1;
      transform: scale(1.1);
    }
  }
  @keyframes bloom-speak {
    0%,
    100% {
      opacity: 0.8;
      transform: scale(1.02);
    }
    50% {
      opacity: 1;
      transform: scale(1.08);
    }
  }
  @keyframes look-around {
    0%,
    18% {
      transform: translate(0, 0);
    }
    28% {
      transform: translate(3px, -1px);
    }
    42% {
      transform: translate(-2px, 1px);
    }
    58% {
      transform: translate(2px, 2px);
    }
    72% {
      transform: translate(-3px, -1px);
    }
    88%,
    100% {
      transform: translate(0, 0);
    }
  }
  @keyframes blink-iris {
    0%,
    90%,
    100% {
      transform: scaleY(1);
      opacity: 1;
    }
    93%,
    95% {
      transform: scaleY(0.12);
      opacity: 0.55;
    }
  }
  @keyframes think-pulse {
    0%,
    100% {
      transform: scale(0.92);
    }
    50% {
      transform: scale(1.08);
    }
  }
  @keyframes speak-iris {
    0%,
    100% {
      transform: scale(1);
    }
    50% {
      transform: scale(1.12);
    }
  }
  @keyframes curious {
    0%,
    100% {
      transform: translate(0, 0) rotate(0deg);
    }
    25% {
      transform: translate(4px, -2px) rotate(2deg);
    }
    75% {
      transform: translate(-3px, 1px) rotate(-2deg);
    }
  }
  @keyframes ring-expand {
    0% {
      transform: scale(0.78);
      opacity: 0.55;
    }
    100% {
      transform: scale(1.35);
      opacity: 0;
    }
  }
  @keyframes smile-talk {
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
  @keyframes mote {
    0%,
    100% {
      opacity: 0.35;
      transform: translateY(0);
    }
    50% {
      opacity: 0.95;
      transform: translateY(-6px);
    }
  }
  @keyframes hop {
    0%,
    100% {
      transform: translateY(0);
    }
    40% {
      transform: translateY(-8px) scale(1.04);
    }
    70% {
      transform: translateY(-2px);
    }
  }
  @keyframes notify-flash {
    0%,
    100% {
      transform: scale(1);
    }
    50% {
      transform: scale(1.18);
    }
  }
</style>
