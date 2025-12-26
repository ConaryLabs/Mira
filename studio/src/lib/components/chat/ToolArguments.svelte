<script lang="ts">
  /**
   * ToolArguments - Structured display of tool call arguments
   *
   * Renders arguments as readable key-value pairs instead of raw JSON
   */

  interface Props {
    args: Record<string, unknown>;
    compact?: boolean;
  }

  let { args, compact = false }: Props = $props();

  // Format a value for display
  function formatValue(value: unknown): { type: string; display: string; isLong: boolean } {
    if (value === null) return { type: 'null', display: 'null', isLong: false };
    if (value === undefined) return { type: 'undefined', display: 'undefined', isLong: false };

    if (typeof value === 'boolean') {
      return { type: 'boolean', display: value.toString(), isLong: false };
    }

    if (typeof value === 'number') {
      return { type: 'number', display: value.toString(), isLong: false };
    }

    if (typeof value === 'string') {
      const isLong = value.length > 80 || value.includes('\n');
      // Truncate for preview
      const display = isLong
        ? value.slice(0, 80) + (value.length > 80 ? '...' : '')
        : value;
      return { type: 'string', display, isLong };
    }

    if (Array.isArray(value)) {
      if (value.length === 0) return { type: 'array', display: '[]', isLong: false };
      const display = `[${value.length} items]`;
      return { type: 'array', display, isLong: true };
    }

    if (typeof value === 'object') {
      const keys = Object.keys(value);
      if (keys.length === 0) return { type: 'object', display: '{}', isLong: false };
      return { type: 'object', display: `{${keys.length} fields}`, isLong: true };
    }

    return { type: 'unknown', display: String(value), isLong: false };
  }

  // Check if value looks like code
  function looksLikeCode(value: string): boolean {
    return value.includes('\n') && (
      value.includes('function') ||
      value.includes('const ') ||
      value.includes('let ') ||
      value.includes('import ') ||
      value.includes('export ') ||
      value.includes('class ') ||
      value.includes('def ') ||
      value.includes('fn ') ||
      value.includes('=>')
    );
  }

  // Check if value looks like a path
  function looksLikePath(key: string, value: string): boolean {
    return (
      key.toLowerCase().includes('path') ||
      key.toLowerCase().includes('file') ||
      value.startsWith('/') ||
      value.includes('/')
    );
  }

  // Get entries with formatted values
  const entries = $derived(
    Object.entries(args).map(([key, value]) => ({
      key,
      value,
      formatted: formatValue(value),
      isCode: typeof value === 'string' && looksLikeCode(value),
      isPath: typeof value === 'string' && looksLikePath(key, value),
    }))
  );

  let expandedKeys = $state<Set<string>>(new Set());

  function toggleExpand(key: string) {
    const newSet = new Set(expandedKeys);
    if (newSet.has(key)) {
      newSet.delete(key);
    } else {
      newSet.add(key);
    }
    expandedKeys = newSet;
  }
</script>

<div class="tool-arguments" class:compact>
  {#each entries as { key, value, formatted, isCode, isPath }}
    <div class="arg-row">
      <span class="arg-key">{key}:</span>

      {#if formatted.isLong || isCode}
        <button class="arg-expand" onclick={() => toggleExpand(key)}>
          <span class="arg-value type-{formatted.type}">{formatted.display}</span>
          <span class="expand-icon">{expandedKeys.has(key) ? '−' : '+'}</span>
        </button>

        {#if expandedKeys.has(key)}
          <div class="arg-expanded">
            {#if isCode}
              <pre class="code-block">{value}</pre>
            {:else if typeof value === 'string'}
              <pre class="string-block">{value}</pre>
            {:else}
              <pre class="json-block">{JSON.stringify(value, null, 2)}</pre>
            {/if}
          </div>
        {/if}
      {:else if isPath}
        <span class="arg-value type-path">{formatted.display}</span>
      {:else}
        <span class="arg-value type-{formatted.type}">{formatted.display}</span>
      {/if}
    </div>
  {/each}
</div>

<style>
  .tool-arguments {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11px;
    font-family: var(--font-mono);
  }

  .tool-arguments.compact {
    gap: 2px;
    font-size: 10px;
  }

  .arg-row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 6px;
    line-height: 1.5;
  }

  .arg-key {
    color: var(--term-text-dim);
    flex-shrink: 0;
  }

  .arg-value {
    color: var(--term-text);
    word-break: break-word;
  }

  .arg-value.type-string {
    color: var(--term-green);
  }

  .arg-value.type-number {
    color: var(--term-orange);
  }

  .arg-value.type-boolean {
    color: var(--term-purple);
  }

  .arg-value.type-null,
  .arg-value.type-undefined {
    color: var(--term-text-dim);
    font-style: italic;
  }

  .arg-value.type-array,
  .arg-value.type-object {
    color: var(--term-cyan);
    font-style: italic;
  }

  .arg-value.type-path {
    color: var(--term-accent);
  }

  .arg-expand {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    font: inherit;
  }

  .arg-expand:hover .expand-icon {
    color: var(--term-accent);
  }

  .expand-icon {
    color: var(--term-text-dim);
    font-size: 12px;
    font-weight: bold;
    width: 14px;
    height: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--term-border);
    border-radius: 3px;
    background: var(--term-bg);
  }

  .arg-expanded {
    width: 100%;
    margin-top: 4px;
    margin-left: 12px;
  }

  .code-block,
  .string-block,
  .json-block {
    margin: 0;
    padding: 8px;
    background: var(--term-bg-secondary);
    border-radius: 4px;
    overflow-x: auto;
    max-height: 200px;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .code-block {
    color: var(--term-text);
  }

  .string-block {
    color: var(--term-green);
  }

  .json-block {
    color: var(--term-text-dim);
  }
</style>
