import DOMPurify from "dompurify";
import { marked } from "marked";

marked.setOptions({
  gfm: true,
  breaks: true,
});

const ALLOWED_TAGS = [
  "p",
  "br",
  "strong",
  "em",
  "b",
  "i",
  "u",
  "s",
  "del",
  "code",
  "pre",
  "blockquote",
  "ul",
  "ol",
  "li",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "a",
  "hr",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
];

const ALLOWED_ATTR = ["href", "title", "target", "rel", "class"];

let hooksReady = false;

function ensureHooks() {
  if (hooksReady || typeof window === "undefined") return;
  hooksReady = true;
  DOMPurify.addHook("afterSanitizeAttributes", (node) => {
    if (node.tagName === "A") {
      node.setAttribute("target", "_blank");
      node.setAttribute("rel", "noopener noreferrer");
    }
  });
}

/** Convert assistant markdown to sanitized HTML for chat bubbles. */
export function renderMarkdown(source: string): string {
  const text = source ?? "";
  if (!text) return "";
  const prepared = text.replace(
    /^(#{1,3}\s+)(Risultato|Tool|TOOL RESULT|Esito)\b/gim,
    "$1$2",
  );
  let html = marked.parse(prepared, { async: false }) as string;
  html = html.replace(/<blockquote>/gi, '<blockquote class="md-callout">');
  if (typeof window === "undefined") return html;
  ensureHooks();
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    ALLOW_DATA_ATTR: false,
  });
}
