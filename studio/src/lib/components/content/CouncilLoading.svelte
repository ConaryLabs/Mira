<script lang="ts">
  import type { CouncilResponses } from '$lib/types/content';
  import { PROVIDER_DISPLAY_NAMES } from '$lib/types/content';

  interface Props {
    partial?: Partial<CouncilResponses>;
  }

  let { partial }: Props = $props();

  // Show which providers we've received so far
  const receivedProviders = $derived(
    partial ? Object.keys(partial).filter(k => partial[k]) : []
  );

  const pendingProviders = $derived(
    ['gpt-5.2', 'opus-4.5', 'gemini-3-pro'].filter(p => !receivedProviders.includes(p))
  );
</script>

<div class="council-loading my-3 rounded border border-[var(--term-border)] bg-[var(--term-bg-secondary)] overflow-hidden">
  <!-- Header -->
  <div class="flex items-center gap-2 px-3 py-2 border-b border-[var(--term-border)] bg-[var(--term-bg)]">
    <span class="text-[var(--term-accent)] font-semibold text-sm">Council</span>
    <span class="text-[var(--term-text-dim)] text-xs animate-pulse">Loading responses...</span>
  </div>

  <!-- Skeleton Cards -->
  <div class="p-2 flex flex-col gap-2">
    {#each pendingProviders as provider (provider)}
      <div class="border border-[var(--term-border)] rounded bg-[var(--term-bg)] p-3">
        <div class="flex items-center gap-2 mb-2">
          <span class="text-[var(--term-text-dim)] font-mono text-xs">[...]</span>
          <span class="text-[var(--term-accent)] font-semibold text-sm">
            {PROVIDER_DISPLAY_NAMES[provider] || provider}
          </span>
        </div>
        <div class="space-y-2">
          <div class="h-3 bg-[var(--term-bg-secondary)] rounded animate-pulse w-3/4"></div>
          <div class="h-3 bg-[var(--term-bg-secondary)] rounded animate-pulse w-1/2"></div>
        </div>
      </div>
    {/each}

    {#if partial}
      {#each receivedProviders as provider (provider)}
        <div class="border border-[var(--term-border)] rounded bg-[var(--term-bg)] p-3">
          <div class="flex items-center gap-2 mb-2">
            <span class="text-[var(--term-success)] font-mono text-xs">[✓]</span>
            <span class="text-[var(--term-accent)] font-semibold text-sm">
              {PROVIDER_DISPLAY_NAMES[provider] || provider}
            </span>
          </div>
          <div class="text-[var(--term-text-dim)] text-sm line-clamp-2">
            {partial[provider]?.slice(0, 100)}...
          </div>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .line-clamp-2 {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
</style>
