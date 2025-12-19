<script lang="ts">
  import ProviderCard from './ProviderCard.svelte';
  import { PROVIDER_DISPLAY_NAMES, type CouncilResponses } from '$lib/types/content';

  interface Props {
    id: string;
    responses: CouncilResponses;
  }

  let { id, responses }: Props = $props();

  let copied = $state(false);

  // Order providers consistently
  const providerOrder = ['gpt-5.2', 'opus-4.5', 'gemini-3-pro'];

  const providers = $derived.by(() => {
    return providerOrder
      .filter(key => responses[key])
      .map((key, index) => ({
        key,
        response: responses[key]!,
        isFirst: index === 0,
      }));
  });

  async function copyAll() {
    try {
      const text = providers
        .map(p => `## ${PROVIDER_DISPLAY_NAMES[p.key] || p.key}\n\n${p.response}`)
        .join('\n\n---\n\n');
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => { copied = false; }, 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }
</script>

<div class="council-view my-3 rounded border border-[var(--term-border)] bg-[var(--term-bg-secondary)] overflow-hidden">
  <!-- Header -->
  <div class="flex items-center justify-between px-3 py-2 border-b border-[var(--term-border)] bg-[var(--term-bg)]">
    <div class="flex items-center gap-2">
      <span class="text-[var(--term-accent)] font-semibold text-sm">Council</span>
      <span class="text-[var(--term-text-dim)] text-xs">
        {providers.map(p => PROVIDER_DISPLAY_NAMES[p.key] || p.key).join(' | ')}
      </span>
    </div>
    <button
      type="button"
      onclick={copyAll}
      class="text-xs px-2 py-0.5 rounded hover:bg-[var(--term-bg-secondary)] text-[var(--term-text-dim)] hover:text-[var(--term-accent)] transition-colors"
    >
      {copied ? 'Copied!' : 'Copy All'}
    </button>
  </div>

  <!-- Provider Cards -->
  <div class="p-2 flex flex-col gap-2">
    {#each providers as provider (provider.key)}
      <ProviderCard
        id={`${id}-${provider.key}`}
        provider={provider.key}
        response={provider.response}
        defaultExpanded={provider.isFirst}
      />
    {/each}
  </div>
</div>
