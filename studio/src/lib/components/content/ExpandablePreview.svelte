<script lang="ts">
  interface Props {
    defaultExpanded?: boolean;
    previewLines?: number;
    showRawToggle?: boolean;
    onToggle?: (expanded: boolean) => void;
  }

  let {
    defaultExpanded = false,
    previewLines = 5,
    showRawToggle = false,
    onToggle,
  }: Props = $props();

  let expanded = $state(defaultExpanded);
  let showRaw = $state(false);

  function toggle() {
    expanded = !expanded;
    onToggle?.(expanded);
  }

  function toggleRaw() {
    showRaw = !showRaw;
  }
</script>

<div class="expandable-preview">
  <!-- Header with expand toggle -->
  <div class="flex items-center gap-2 w-full px-2 py-1">
    <button
      type="button"
      onclick={toggle}
      class="flex items-center gap-2 text-left hover:text-[var(--term-accent)] transition-colors cursor-pointer"
    >
      <span class="text-[var(--term-text-dim)] font-mono text-xs select-none">
        [{expanded ? '-' : '+'}]
      </span>
      <slot name="header" />
    </button>
    {#if showRawToggle}
      <button
        type="button"
        onclick={toggleRaw}
        class="ml-auto text-xs text-[var(--term-text-dim)] hover:text-[var(--term-accent)] px-1"
      >
        [{showRaw ? 'pretty' : 'raw'}]
      </button>
    {/if}
  </div>

  <!-- Content area -->
  <div
    class="content-area mt-1 pl-6 border-l border-[var(--term-border)] overflow-hidden transition-all duration-200"
    class:expanded
  >
    {#if showRaw}
      <slot name="raw" />
    {:else if expanded}
      <slot name="full" />
    {:else}
      <slot name="preview" {previewLines} />
    {/if}
  </div>
</div>

<style>
  .content-area {
    max-height: 150px;
  }

  .content-area.expanded {
    max-height: none;
  }

  /* Smooth transition for expand/collapse */
  .content-area:not(.expanded) {
    mask-image: linear-gradient(to bottom, black 70%, transparent 100%);
    -webkit-mask-image: linear-gradient(to bottom, black 70%, transparent 100%);
  }
</style>
