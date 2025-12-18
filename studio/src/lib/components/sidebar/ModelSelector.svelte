<script lang="ts">
  import { settings, modelProviderOptions, type ModelProvider } from '$lib/stores/settings';

  function handleChange(event: Event) {
    const target = event.target as HTMLSelectElement;
    settings.setModelProvider(target.value as ModelProvider);
  }
</script>

<div class="space-y-2">
  <label class="block text-xs text-[var(--term-text-dim)] uppercase tracking-wide">
    Model
  </label>
  <select
    value={$settings.modelProvider}
    onchange={handleChange}
    class="w-full bg-[var(--term-bg)] text-[var(--term-text)] text-sm font-mono px-2 py-1.5 rounded border border-[var(--term-border)] focus:border-[var(--term-accent)] focus:outline-none"
  >
    {#each modelProviderOptions as option}
      <option value={option.value}>{option.label}</option>
    {/each}
  </select>
  <p class="text-xs text-[var(--term-text-dim)]">
    {modelProviderOptions.find(o => o.value === $settings.modelProvider)?.description || ''}
  </p>
</div>
