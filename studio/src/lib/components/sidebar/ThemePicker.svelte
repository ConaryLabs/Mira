<script lang="ts">
  import { currentTheme, themes, themeNames, type ThemeName } from '$lib/stores/theme';

  const themeLabels: Record<ThemeName, string> = {
    'terminal-dark': 'Dark',
    'terminal-retro': 'Retro',
    'terminal-modern': 'Modern',
    'terminal-neon': 'Neon',
    'corporate-light': 'Office',
  };
</script>

<div class="space-y-2">
  <label class="block text-xs text-[var(--term-text-dim)] uppercase tracking-wide">
    Theme
  </label>
  <div class="grid grid-cols-2 gap-2">
    {#each themeNames as themeName}
      {@const colors = themes[themeName]}
      <button
        onclick={() => currentTheme.set(themeName)}
        class="relative p-2 rounded border-2 transition-all {$currentTheme === themeName
          ? 'border-[var(--term-accent)]'
          : 'border-[var(--term-border)] hover:border-[var(--term-text-dim)]'}"
        style="background: {colors.bg}"
      >
        <!-- Color preview dots -->
        <div class="flex gap-1 mb-1">
          <span class="w-2 h-2 rounded-full" style="background: {colors.prompt}"></span>
          <span class="w-2 h-2 rounded-full" style="background: {colors.accent}"></span>
          <span class="w-2 h-2 rounded-full" style="background: {colors.success}"></span>
          <span class="w-2 h-2 rounded-full" style="background: {colors.error}"></span>
        </div>
        <!-- Label -->
        <span class="text-xs font-mono" style="color: {colors.text}">
          {themeLabels[themeName]}
        </span>
        <!-- Active indicator -->
        {#if $currentTheme === themeName}
          <span class="absolute -top-1 -right-1 w-3 h-3 bg-[var(--term-accent)] rounded-full flex items-center justify-center">
            <span class="text-[8px] text-[var(--term-bg)]">✓</span>
          </span>
        {/if}
      </button>
    {/each}
  </div>
</div>
