<script lang="ts">
  interface Props {
    onSubmit: (message: string) => void;
    disabled?: boolean;
  }

  let { onSubmit, disabled = false }: Props = $props();

  let message = $state('');
  let inputEl: HTMLTextAreaElement;

  function handleSubmit() {
    const trimmed = message.trim();
    if (trimmed && !disabled) {
      onSubmit(trimmed);
      message = '';
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      handleSubmit();
    }
  }

  function adjustHeight() {
    if (inputEl) {
      inputEl.style.height = 'auto';
      inputEl.style.height = Math.min(inputEl.scrollHeight, 200) + 'px';
    }
  }
</script>

<div class="deliberation-input">
  <div class="input-header">
    <svg class="council-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M23 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75" />
    </svg>
    <span class="header-text">Ask the Council</span>
    <span class="model-list">GPT-5.2 + Opus 4.5 + Gemini 3 Pro</span>
  </div>

  <div class="input-area">
    <textarea
      bind:this={inputEl}
      bind:value={message}
      onkeydown={handleKeydown}
      oninput={adjustHeight}
      placeholder="Ask a question for the council to deliberate on..."
      rows="3"
      {disabled}
    ></textarea>
  </div>

  <div class="input-footer">
    <span class="hint">
      <kbd>Cmd</kbd>+<kbd>Enter</kbd> to submit
    </span>
    <button
      class="submit-btn"
      onclick={handleSubmit}
      disabled={disabled || !message.trim()}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
      </svg>
      Start Deliberation
    </button>
  </div>
</div>

<style>
  .deliberation-input {
    display: flex;
    flex-direction: column;
    padding: 16px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 8px;
    margin: 12px;
  }

  .input-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }

  .council-icon {
    width: 20px;
    height: 20px;
    color: var(--term-accent);
  }

  .header-text {
    font-size: 14px;
    font-weight: 600;
    color: var(--term-text);
  }

  .model-list {
    margin-left: auto;
    font-size: 11px;
    color: var(--term-text-dim);
  }

  .input-area {
    margin-bottom: 12px;
  }

  textarea {
    width: 100%;
    padding: 12px;
    font-family: inherit;
    font-size: 13px;
    line-height: 1.5;
    color: var(--term-text);
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 6px;
    resize: none;
    transition: border-color 0.15s;
  }

  textarea:focus {
    outline: none;
    border-color: var(--term-accent);
  }

  textarea::placeholder {
    color: var(--term-text-dim);
  }

  textarea:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .input-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .hint {
    font-size: 11px;
    color: var(--term-text-dim);
    display: flex;
    align-items: center;
    gap: 4px;
  }

  kbd {
    padding: 2px 5px;
    font-family: var(--font-mono);
    font-size: 10px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 3px;
  }

  .submit-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    font-size: 13px;
    font-weight: 500;
    color: white;
    background: var(--term-accent);
    border: none;
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .submit-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .submit-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .submit-btn svg {
    width: 16px;
    height: 16px;
  }
</style>
