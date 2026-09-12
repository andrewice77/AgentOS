<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    description?: string;
    breadcrumb?: string;
    actions?: Snippet;
  }

  let { title, description = "", breadcrumb = "", actions }: Props = $props();
</script>

<header class="page-header">
  <div class="page-header-text">
    {#if breadcrumb}
      <p class="breadcrumb" aria-label="Percorso">{breadcrumb}</p>
    {/if}
    <h2>{title}</h2>
    {#if description}
      <p class="description">{description}</p>
    {/if}
  </div>
  {#if actions}
    <div class="page-actions">
      {@render actions()}
    </div>
  {/if}
</header>

<style>
  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: var(--space-lg);
    margin-bottom: var(--space-md);
    padding-bottom: var(--space-md);
    border-bottom: 1px solid var(--line);
  }

  .breadcrumb {
    margin: 0 0 0.2rem;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  h2 {
    margin: 0;
    font-family: var(--font-display);
    font-weight: 500;
    font-size: 1.35rem;
    color: var(--ink);
  }

  .description {
    margin: 0.35rem 0 0;
    color: var(--muted);
    font-size: 0.88rem;
    line-height: 1.45;
    max-width: 52rem;
  }

  .page-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-xs);
    flex-shrink: 0;
  }
</style>
