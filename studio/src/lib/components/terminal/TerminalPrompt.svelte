<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    onSend: (message: string) => void;
    onCancel?: () => void;
    disabled?: boolean;
    isStreaming?: boolean;
    placeholder?: string;
  }

  let {
    onSend,
    onCancel,
    disabled = false,
    isStreaming = false,
    placeholder = 'Enter command...'
  }: Props = $props();

  let inputValue = $state('');
  let textareaEl: HTMLTextAreaElement;

  // Auto-focus on mount
  onMount(() => {
    textareaEl?.focus();
  });

  // Expose focus method for keyboard shortcuts
  export function focus() {
    textareaEl?.focus();
  }

  function handleKeydown(event: KeyboardEvent) {
    // Escape to cancel streaming
    if (event.key === 'Escape' && isStreaming && onCancel) {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      submit();
    }
  }

  function submit() {
    const content = inputValue.trim();
    if (!content || disabled) return;
    onSend(content);
    inputValue = '';
    // Reset textarea height
    if (textareaEl) {
      textareaEl.style.height = 'auto';
    }
  }

  function handleInput() {
    // Auto-resize textarea
    if (textareaEl) {
      textareaEl.style.height = 'auto';
      textareaEl.style.height = Math.min(textareaEl.scrollHeight, 200) + 'px';
    }
  }
</script>

<div class="flex items-start gap-2 px-4 py-3 bg-[var(--term-bg-secondary)] border-t border-[var(--term-border)]" role="form" aria-label="Message input">
  <span class="terminal-prompt-char text-[var(--term-prompt)] font-mono font-bold text-lg select-none pt-0.5" aria-hidden="true">{'>'}</span>
  <textarea
    bind:this={textareaEl}
    bind:value={inputValue}
    onkeydown={handleKeydown}
    oninput={handleInput}
    aria-label="Type your message"
    {placeholder}
    {disabled}
    rows="1"
    class="flex-1 bg-transparent text-[var(--term-text)] font-mono text-sm resize-none outline-none placeholder:text-[var(--term-text-dim)] disabled:opacity-50"
  ></textarea>
  {#if isStreaming && onCancel}
    <button
      onclick={onCancel}
      aria-label="Cancel streaming (Escape)"
      class="text-[var(--term-error)] font-mono text-sm hover:text-[var(--term-text)] transition-colors px-2"
      title="Cancel (Esc)"
    >
      [cancel]
    </button>
  {:else}
    <button
      onclick={submit}
      disabled={!inputValue.trim() || disabled}
      aria-label="Send message"
      class="text-[var(--term-accent)] font-mono text-sm hover:text-[var(--term-text)] disabled:opacity-30 disabled:cursor-not-allowed transition-colors px-2"
    >
      [send]
    </button>
  {/if}
</div>
