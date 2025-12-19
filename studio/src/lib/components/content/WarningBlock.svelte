<script lang="ts">
  interface Props {
    message: string;
  }

  let { message }: Props = $props();
</script>

<div class="warning-block my-2 p-3 rounded border-l-4 border-[var(--term-warning)] bg-[var(--term-bg-secondary)]">
  <div class="flex items-start gap-2">
    <span class="text-[var(--term-warning)] font-bold text-sm">warning</span>
  </div>
  <p class="mt-1 text-sm text-[var(--term-text)]">{message}</p>
</div>
