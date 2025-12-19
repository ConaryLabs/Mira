<script lang="ts">
  import { onDestroy } from 'svelte';
  import { DebouncedParser } from '$lib/parser/DebouncedParser.svelte';
  import { ContentBlock } from '$lib/components/content';

  interface Props {
    content: string;
    blockId: string;
  }

  let { content, blockId }: Props = $props();

  // Create parser instance for this block
  const parser = new DebouncedParser(blockId);

  // Update parser when content changes
  $effect(() => {
    parser.update(content);
  });

  // Cleanup on unmount
  onDestroy(() => {
    parser.destroy();
  });
</script>

<!-- Render parsed segments -->
{#each parser.result.segments as segment (segment.id)}
  <ContentBlock content={segment} />
{/each}

<!-- Show subtle indicator when parse is pending (optional, can remove if too noisy) -->
{#if parser.isPending && parser.result.segments.length === 0}
  <span class="text-[var(--term-text-dim)]">...</span>
{/if}
