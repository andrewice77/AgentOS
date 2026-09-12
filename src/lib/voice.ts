/** Client-side voice helpers: Web Speech + Piper/Audio8 via Tauri. */

import { markdownToSpeechText } from "$lib/speechText";

export type VoiceSettings = {
  enabled?: boolean;
  auto_read_replies?: boolean;
  engine?: string;
  voice?: string;
  rate?: number;
  volume?: number;
  stt_enabled?: boolean;
  audio8_voice?: string;
  audio8_device?: string;
  audio8_base_url?: string;
};

function sanitize(text: string): string {
  return markdownToSpeechText(text);
}

export function speakSystem(text: string, cfg: VoiceSettings): Promise<void> {
  return new Promise((resolve) => {
    if (typeof window === "undefined" || !window.speechSynthesis) {
      resolve();
      return;
    }
    const clean = sanitize(text);
    if (!clean) {
      resolve();
      return;
    }
    window.speechSynthesis.cancel();
    const u = new SpeechSynthesisUtterance(clean);
    u.rate = Math.min(2, Math.max(0.5, cfg.rate ?? 1));
    u.volume = Math.min(1, Math.max(0, cfg.volume ?? 1));
    u.lang = "it-IT";
    const voices = window.speechSynthesis.getVoices();
    const hint = (cfg.voice ?? "").toLowerCase();
    const pick =
      voices.find((v) => hint && v.name.toLowerCase().includes(hint)) ||
      voices.find((v) => v.lang.toLowerCase().startsWith("it")) ||
      voices.find((v) => v.lang.toLowerCase().includes("it"));
    if (pick) u.voice = pick;
    u.onend = () => resolve();
    u.onerror = () => resolve();
    window.speechSynthesis.speak(u);
  });
}

/**
 * Legge una risposta assistente.
 * Per piper/audio8 propaga gli errori (niente fallback silenzioso).
 */
export async function speakAssistantReply(
  text: string,
  cfg: VoiceSettings | null | undefined,
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>,
  opts?: { force?: boolean },
): Promise<void> {
  if (!cfg?.enabled && !opts?.force) return;
  if (!opts?.force && cfg?.auto_read_replies === false) return;
  const clean = sanitize(text);
  if (!clean) return;

  const engine = cfg?.engine ?? "system";
  if (engine === "system") {
    await speakSystem(clean, cfg ?? {});
    return;
  }

  await invoke("speak_text", { text: clean });
}

/** Avvia la lettura in background subito a fine messaggio. */
export function startReadingReply(
  text: string,
  cfg: VoiceSettings | null | undefined,
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>,
  hooks?: {
    onStart?: () => void;
    onDone?: () => void;
    onError?: (err: string) => void;
    force?: boolean;
  },
): void {
  hooks?.onStart?.();
  void speakAssistantReply(text, cfg, invoke, { force: hooks?.force })
    .then(() => hooks?.onDone?.())
    .catch((e) => {
      hooks?.onError?.(String(e));
      hooks?.onDone?.();
    });
}

export async function stopSpeaking(
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>,
): Promise<void> {
  try {
    if (typeof window !== "undefined" && window.speechSynthesis) {
      window.speechSynthesis.cancel();
    }
    await invoke("stop_speech");
  } catch {
    /* ignore */
  }
}
