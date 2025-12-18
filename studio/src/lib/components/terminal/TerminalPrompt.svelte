<script lang="ts">
  interface Props {
    onSend: (message: string) => void;
    disabled?: boolean;
    placeholder?: string;
  }

  let { onSend, disabled = false, placeholder = 'Enter command...' }: Props = $props();

  let inputValue = $state('');
  let textareaEl: HTMLTextAreaElement;

  function handleKeydown(event: KeyboardEvent) {
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

<div class="flex items-start gap-2 px-4 py-3 bg-[var(--term-bg-secondary)] border-t border-[var(--term-border)]">
  <span class="terminal-prompt-char text-[var(--term-prompt)] font-mono font-bold text-lg select-none pt-0.5">{'>'}</span>
  <textarea
    bind:this={textareaEl}
    bind:value={inputValue}
    onkeydown={handleKeydown}
    oninput={handleInput}
    {placeholder}
    {disabled}
    rows="1"
    class="flex-1 bg-transparent text-[var(--term-text)] font-mono text-sm resize-none outline-none placeholder:text-[var(--term-text-dim)] disabled:opacity-50"
  ></textarea>
  <button
    onclick={submit}
    disabled={!inputValue.trim() || disabled}
    class="text-[var(--term-accent)] font-mono text-sm hover:text-[var(--term-text)] disabled:opacity-30 disabled:cursor-not-allowed transition-colors px-2"
  >
    [send]
  </button>
</div>
