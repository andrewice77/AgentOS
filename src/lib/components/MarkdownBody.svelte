<script lang="ts">
  import { renderMarkdown } from "$lib/markdown";

  let { source = "" }: { source?: string } = $props();
  let html = $derived(renderMarkdown(source));
</script>

<div class="md">{@html html}</div>

<style>
  .md {
    line-height: 1.55;
    font-size: 0.94rem;
    overflow-wrap: anywhere;
    word-break: break-word;
    color: var(--ink);
  }

  .md :global(> :first-child) {
    margin-top: 0;
  }

  .md :global(> :last-child) {
    margin-bottom: 0;
  }

  .md :global(p) {
    margin: 0.5em 0;
    white-space: normal;
  }

  .md :global(h1),
  .md :global(h2),
  .md :global(h3),
  .md :global(h4) {
    margin: 0.95em 0 0.4em;
    font-family: var(--font-display);
    font-weight: 650;
    line-height: 1.25;
    color: var(--ink);
    letter-spacing: -0.01em;
  }

  .md :global(h1) {
    font-size: 1.28rem;
    border-bottom: 1px solid var(--line);
    padding-bottom: 0.25em;
  }
  .md :global(h2) {
    font-size: 1.14rem;
    color: color-mix(in srgb, var(--ink) 92%, var(--accent));
  }
  .md :global(h3) {
    font-size: 1.02rem;
    font-weight: 600;
  }
  .md :global(h4) {
    font-size: 0.96rem;
    font-weight: 600;
    color: var(--muted);
  }

  .md :global(strong) {
    font-weight: 650;
    color: var(--ink);
  }

  .md :global(ul),
  .md :global(ol) {
    margin: 0.55em 0;
    padding-left: 1.35em;
  }

  .md :global(li) {
    margin: 0.32em 0;
  }

  .md :global(li > ul),
  .md :global(li > ol) {
    margin: 0.2em 0 0.3em;
  }

  .md :global(code) {
    font-family: ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace;
    font-size: 0.86em;
    background: color-mix(in srgb, var(--bg0) 70%, transparent);
    color: var(--accent);
    padding: 0.1em 0.4em;
    border-radius: 6px;
    border: 1px solid var(--line);
  }

  .md :global(pre) {
    margin: 0.7em 0;
    padding: 0.75rem 0.85rem;
    overflow: auto;
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg0) 82%, #000);
    border: 1px solid var(--line);
  }

  .md :global(pre code) {
    padding: 0;
    background: transparent;
    border: 0;
    color: var(--ink);
    font-size: 0.82rem;
    white-space: pre;
  }

  .md :global(blockquote),
  .md :global(.md-callout),
  .md :global(.md-tool-result) {
    margin: 0.65em 0;
    padding: 0.55em 0.75em;
    border-left: 3px solid color-mix(in srgb, var(--accent) 70%, transparent);
    border-radius: 0 8px 8px 0;
    background: color-mix(in srgb, var(--accent-soft) 35%, transparent);
    color: var(--ink);
  }

  .md :global(blockquote p) {
    margin: 0.25em 0;
  }

  .md :global(a) {
    color: var(--accent-2);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .md :global(hr) {
    margin: 0.9em 0;
    border: 0;
    border-top: 1px solid var(--line);
  }

  .md :global(table) {
    width: 100%;
    border-collapse: collapse;
    margin: 0.65em 0;
    font-size: 0.86rem;
    display: block;
    overflow-x: auto;
  }

  .md :global(th),
  .md :global(td) {
    border: 1px solid var(--line);
    padding: 0.4em 0.55em;
    text-align: left;
  }

  .md :global(th) {
    background: color-mix(in srgb, var(--bg0) 55%, transparent);
    font-weight: 600;
  }

  .md :global(tr:nth-child(even) td) {
    background: color-mix(in srgb, var(--bg2) 40%, transparent);
  }
</style>
