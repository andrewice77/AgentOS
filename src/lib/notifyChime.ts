/** Short in-app chime so a due reminder is audible even if the OS toast is muted. */

let ctx: AudioContext | null = null;

function audioContext(): AudioContext | null {
  const AC =
    window.AudioContext ||
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!AC) return null;
  if (!ctx) ctx = new AC();
  return ctx;
}

export function playReminderChime(): void {
  try {
    const c = audioContext();
    if (!c) return;
    void c.resume();
    const t0 = c.currentTime + 0.02;
    const notes = [
      { f: 784, t: 0, d: 0.11, g: 0.16 },
      { f: 1046.5, t: 0.11, d: 0.16, g: 0.18 },
      { f: 1318.5, t: 0.26, d: 0.32, g: 0.14 },
    ];
    for (const n of notes) {
      const osc = c.createOscillator();
      const gain = c.createGain();
      osc.type = "sine";
      osc.frequency.value = n.f;
      gain.gain.setValueAtTime(0.0001, t0 + n.t);
      gain.gain.exponentialRampToValueAtTime(n.g, t0 + n.t + 0.018);
      gain.gain.exponentialRampToValueAtTime(0.0001, t0 + n.t + n.d);
      osc.connect(gain);
      gain.connect(c.destination);
      osc.start(t0 + n.t);
      osc.stop(t0 + n.t + n.d + 0.03);
    }
  } catch {
    /* ignore autoplay / closed context */
  }
}
