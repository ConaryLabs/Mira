<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { currentTheme } from '$lib/stores/theme';

  const { children } = $props();

  // Apply theme on mount
  onMount(() => {
    currentTheme.init();
  });
</script>

<div class="h-screen-safe w-screen overflow-hidden bg-[var(--term-bg)]" data-theme={$currentTheme}>
  {@render children()}
</div>
