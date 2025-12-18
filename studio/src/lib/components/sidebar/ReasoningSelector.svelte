<script lang="ts">
  import { settings, reasoningOptions, type ReasoningEffort } from '$lib/stores/settings';

  function handleChange(event: Event) {
    const target = event.target as HTMLSelectElement;
    settings.setReasoningEffort(target.value as ReasoningEffort);
  }
</script>

<div class="space-y-2">
  <label class="block text-xs text-[var(--term-text-dim)] uppercase tracking-wide">
    Reasoning
  </label>
  <select
    value={$settings.reasoningEffort}
    onchange={handleChange}
    class="w-full bg-[var(--term-bg)] text-[var(--term-text)] text-sm font-mono px-2 py-1.5 rounded border border-[var(--term-border)] focus:border-[var(--term-accent)] focus:outline-none"
  >
    {#each reasoningOptions as option}
      <option value={option.value}>{option.label}</option>
    {/each}
  </select>
  <p class="text-xs text-[var(--term-text-dim)]">
    {reasoningOptions.find(o => o.value === $settings.reasoningEffort)?.description || ''}
  </p>
</div>
