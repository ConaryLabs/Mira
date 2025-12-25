<script lang="ts">
  /**
   * ArtifactCard - Preview card for a single artifact
   *
   * Shows title, type indicator, preview, and actions
   */

  import type { Artifact } from '$lib/stores/artifacts.svelte';
  import { artifactViewer } from '$lib/stores/artifacts.svelte';
  import { layoutStore } from '$lib/stores/layout.svelte';

  interface Props {
    artifact: Artifact;
  }

  let { artifact }: Props = $props();

  // Action icons/colors
  const actionStyles: Record<string, { icon: string; color: string }> = {
    modified: { icon: 'M', color: 'var(--term-warning)' },
    created: { icon: '+', color: 'var(--term-success)' },
    read: { icon: 'R', color: 'var(--term-cyan)' },
    write: { icon: 'W', color: 'var(--term-purple)' },
  };

  const style = $derived(actionStyles[artifact.action] || actionStyles.read);

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
  }

  function handleClick() {
    artifactViewer.open(artifact);
  }

  function jumpToTool(e: MouseEvent) {
    e.stopPropagation();
    if (artifact.sourceCallId) {
      // Focus timeline and scroll to tool call
      layoutStore.setDrawerTab('timeline');
    }
  }
</script>

<button
  class="artifact-card"
  onclick={handleClick}
>
  <div class="card-header">
    <span class="action-badge" style="color: {style.color}">
      [{style.icon}]
    </span>
    <span class="title">{artifact.title}</span>
    <span class="size">{formatBytes(artifact.totalBytes)}</span>
  </div>

  {#if artifact.path}
    <div class="path">{artifact.path}</div>
  {/if}

  <pre class="preview">{artifact.preview}</pre>

  {#if artifact.sourceToolName}
    <div class="card-footer">
      <button class="source-link" onclick={jumpToTool}>
        via {artifact.sourceToolName}
      </button>
      {#if artifact.language}
        <span class="language">{artifact.language}</span>
      {/if}
    </div>
  {/if}
</button>

<style>
  .artifact-card {
    display: block;
    width: 100%;
    text-align: left;
    padding: 10px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 6px;
    cursor: pointer;
    transition: border-color 0.15s ease;
  }

  .artifact-card:hover {
    border-color: var(--term-accent);
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }

  .action-badge {
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: bold;
  }

  .title {
    font-size: 13px;
    font-weight: 500;
    color: var(--term-text);
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .size {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--term-text-dim);
  }

  .path {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--term-text-dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: 6px;
  }

  .preview {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--term-text-dim);
    background: var(--term-bg);
    padding: 6px;
    border-radius: 4px;
    margin: 0;
    max-height: 80px;
    overflow: hidden;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .card-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 6px;
    font-size: 10px;
  }

  .source-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--term-accent);
    cursor: pointer;
    font-size: 10px;
    font-family: inherit;
  }

  .source-link:hover {
    text-decoration: underline;
  }

  .language {
    color: var(--term-text-dim);
    text-transform: uppercase;
    font-weight: 500;
  }
</style>
