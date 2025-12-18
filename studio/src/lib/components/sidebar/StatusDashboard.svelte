<script lang="ts">
  import type { StatusResponse } from '$lib/api/client';

  interface Props {
    status: StatusResponse | null;
  }

  let { status }: Props = $props();

  interface StatusItem {
    label: string;
    active: boolean;
    description: string;
  }

  const items = $derived<StatusItem[]>([
    {
      label: 'API',
      active: status?.status === 'ok',
      description: status?.status === 'ok' ? 'Connected' : 'Disconnected',
    },
    {
      label: 'Database',
      active: status?.database ?? false,
      description: status?.database ? 'Ready' : 'Unavailable',
    },
    {
      label: 'Semantic',
      active: status?.semantic_search ?? false,
      description: status?.semantic_search ? 'Available' : 'Disabled',
    },
  ]);
</script>

<div class="space-y-2">
  <label class="block text-xs text-[var(--term-text-dim)] uppercase tracking-wide">
    Status
  </label>
  <div class="space-y-1">
    {#each items as item}
      <div class="flex items-center justify-between text-sm font-mono">
        <span class="text-[var(--term-text-dim)]">{item.label}</span>
        <span class="flex items-center gap-1.5">
          <span
            class="w-2 h-2 rounded-full {item.active ? 'bg-[var(--term-success)]' : 'bg-[var(--term-error)]'}"
          ></span>
          <span class="{item.active ? 'text-[var(--term-success)]' : 'text-[var(--term-text-dim)]'}">
            {item.description}
          </span>
        </span>
      </div>
    {/each}
  </div>
</div>
