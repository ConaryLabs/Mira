<script lang="ts">
  interface Props {
    message: string;
    code?: string;
  }

  let { message, code }: Props = $props();
</script>

<div class="error-block my-2 p-3 rounded border-l-4 border-[var(--term-error)] bg-[var(--term-bg-secondary)]">
  <div class="flex items-start gap-2">
    <span class="text-[var(--term-error)] font-bold text-sm">error</span>
    {#if code}
      <span class="text-[var(--term-error)] text-sm font-mono">[{code}]</span>
    {/if}
  </div>
  <p class="mt-1 text-sm text-[var(--term-text)]">{message}</p>
</div>
