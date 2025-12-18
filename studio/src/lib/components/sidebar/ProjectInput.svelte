<script lang="ts">
  import { settings } from '$lib/stores/settings';

  let showHistory = $state(false);
  let inputValue = $state($settings.projectPath);

  function handleBlur() {
    if (inputValue !== $settings.projectPath) {
      settings.setProjectPath(inputValue);
    }
    setTimeout(() => showHistory = false, 150);
  }

  function handleFocus() {
    showHistory = true;
  }

  function selectProject(path: string) {
    inputValue = path;
    settings.setProjectPath(path);
    showHistory = false;
  }

  function removeProject(event: Event, path: string) {
    event.stopPropagation();
    settings.removeFromHistory(path);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      (event.target as HTMLInputElement).blur();
    }
    if (event.key === 'Escape') {
      inputValue = $settings.projectPath;
      showHistory = false;
    }
  }

  // Sync with external changes
  $effect(() => {
    inputValue = $settings.projectPath;
  });
</script>

<div class="space-y-2 relative">
  <label class="block text-xs text-[var(--term-text-dim)] uppercase tracking-wide">
    Project
  </label>
  <input
    type="text"
    bind:value={inputValue}
    onblur={handleBlur}
    onfocus={handleFocus}
    onkeydown={handleKeydown}
    class="w-full bg-[var(--term-bg)] text-[var(--term-text)] text-sm font-mono px-2 py-1.5 rounded border border-[var(--term-border)] focus:border-[var(--term-accent)] focus:outline-none"
    placeholder="/path/to/project"
  />

  {#if showHistory && $settings.projectHistory.length > 0}
    <div class="absolute left-0 right-0 mt-1 bg-[var(--term-bg-secondary)] border border-[var(--term-border)] rounded shadow-lg z-10 max-h-48 overflow-y-auto">
      {#each $settings.projectHistory as path}
        <button
          onclick={() => selectProject(path)}
          class="w-full flex items-center justify-between px-2 py-1.5 text-left text-sm font-mono hover:bg-[var(--term-bg)] transition-colors group"
        >
          <span class="text-[var(--term-text)] truncate">{path}</span>
          <span
            role="button"
            tabindex="-1"
            onclick={(e) => removeProject(e, path)}
            onkeydown={(e) => e.key === 'Enter' && removeProject(e, path)}
            class="text-[var(--term-text-dim)] hover:text-[var(--term-error)] opacity-0 group-hover:opacity-100 transition-opacity"
          >
            ×
          </span>
        </button>
      {/each}
    </div>
  {/if}
</div>
