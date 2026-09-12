/** Plain text for TTS — strips markdown and applies Italian phonetic hints. */

export const SPEECH_TEXT_LIMIT = 1800;

const PHONETIC_REPLACEMENTS: ReadonlyArray<[RegExp, string]> = [
  [/\bAgentOS\b/gi, "Eigent os"],
  [/\bOllama\b/gi, "Olàma"],
  [/\bWhisper\b/gi, "Uìsper"],
  [/\bAudio8\b/gi, "Àudio otto"],
  [/\bTTS\b/g, "ti ti esse"],
  [/\bSTT\b/g, "esse ti ti"],
  [/\bMCP\b/g, "eme ci pi"],
  [/\bAPI\b/g, "a pi ai"],
  [/\bGPU\b/g, "gi pi u"],
  [/\bCPU\b/g, "ci pi u"],
  [/\bONNX\b/gi, "on ix"],
  [/\bHTTP\b/gi, "acce ti ti pi"],
  [/\bURL\b/g, "u ar el"],
];

function isSpeakableInlineCode(code: string): boolean {
  const c = code.trim();
  if (!c || c.length > 48) return false;
  if (/[{}\[\]();=<>/\\|]/.test(c)) return false;
  if (/^\s*(import|export|function|const|let|var|class|def|return|if|else|for|while|async|await)\b/.test(c)) {
    return false;
  }
  return /^[\p{L}\p{N}\s._-]+$/u.test(c);
}

function applyPhonetic(text: string): string {
  let out = text;
  for (const [pattern, replacement] of PHONETIC_REPLACEMENTS) {
    out = out.replace(pattern, replacement);
  }
  return out;
}

function stripCodeFencesLineWise(text: string): string {
  let inCode = false;
  const kept: string[] = [];
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (trimmed.startsWith("```") || trimmed.startsWith("~~~")) {
      inCode = !inCode;
      continue;
    }
    if (inCode) continue;
    kept.push(line);
  }
  return kept.join("\n");
}

/** Convert assistant markdown to speakable Italian plain text. */
export function markdownToSpeechText(source: string): string {
  let text = source ?? "";
  if (!text.trim()) return "";

  text = text.replace(/<[^>]+>/g, " ");
  text = stripCodeFencesLineWise(text);
  text = text.replace(/```[\s\S]*?```/g, " ");
  text = text.replace(/~~~[\s\S]*?~~~/g, " ");

  text = text.replace(/!\[([^\]]*)\]\([^)]+\)/g, (_, alt: string) => alt.trim() || " ");
  text = text.replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");
  text = text.replace(/\[([^\]]+)\]\[[^\]]*\]/g, "$1");
  text = text.replace(/<https?:\/\/[^>]+>/gi, " ");

  text = text.replace(/`([^`]+)`/g, (_, code: string) =>
    isSpeakableInlineCode(code) ? code.trim() : " ",
  );

  text = text.replace(/^#{1,6}\s+/gm, "");
  text = text.replace(/^>\s?/gm, "");
  text = text.replace(/^[\s]*[-*+]\s+/gm, "");
  text = text.replace(/^[\s]*\d+\.\s+/gm, "");
  text = text.replace(/^[\s]*([-*_]){3,}[\s]*$/gm, " ");

  text = text.replace(/^[^\n]*\|[^\n]*$/gm, (line) => {
    if (/^\s*\|?[\s:-]+\|[\s|:-]+\|?\s*$/.test(line)) return " ";
    return line.replace(/\|/g, ", ").replace(/^[\s|]+|[\s|]+$/g, "");
  });

  text = text.replace(/\*\*\*([^*]+)\*\*\*/g, "$1");
  text = text.replace(/___([^_]+)___/g, "$1");
  text = text.replace(/\*\*([^*]+)\*\*/g, "$1");
  text = text.replace(/__([^_]+)__/g, "$1");
  text = text.replace(/\*([^*\n]+)\*/g, "$1");
  text = text.replace(/_([^_\n]+)_/g, "$1");
  text = text.replace(/~~([^~]+)~~/g, "$1");

  text = text.replace(/[#*`~]/g, " ");
  text = applyPhonetic(text);
  text = text.replace(/\s+/g, " ").trim();

  return text.slice(0, SPEECH_TEXT_LIMIT);
}
