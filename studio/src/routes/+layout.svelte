<script lang="ts">
  import '../app.css';

  const { children } = $props();
</script>

<div class="h-screen w-screen overflow-hidden bg-[var(--chat-bg)]">
  {@render children()}
</div>
