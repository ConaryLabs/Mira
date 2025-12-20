<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    isOpen: boolean;
    filename?: string;
    language: string;
    code: string;
    onClose: () => void;
  }

  let {
    isOpen,
    filename,
    language,
    code,
    onClose,
  }: Props = $props();

  let copied = $state(false);
  let searchQuery = $state('');
  let searchMatches = $state<number[]>([]);
  let currentMatch = $state(0);
  let codeEl: HTMLPreElement;

  // Language display names
  const languageNames: Record<string, string> = {
    ts: 'TypeScript',
    typescript: 'TypeScript',
    js: 'JavaScript',
    javascript: 'JavaScript',
    rs: 'Rust',
    rust: 'Rust',
    py: 'Python',
    python: 'Python',
    sh: 'Shell',
    bash: 'Bash',
    json: 'JSON',
    html: 'HTML',
    css: 'CSS',
    svelte: 'Svelte',
    sql: 'SQL',
    toml: 'TOML',
    yaml: 'YAML',
    yml: 'YAML',
    md: 'Markdown',
    markdown: 'Markdown',
    text: 'Plain Text',
  };

  const displayLang = $derived(languageNames[language.toLowerCase()] || language);
  const lines = $derived(code.split('\n'));
  const lineCount = $derived(lines.length);

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(code);
      copied = true;
      setTimeout(() => { copied = false; }, 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
    }
    // Cmd/Ctrl + F for search
    if ((event.metaKey || event.ctrlKey) && event.key === 'f') {
      event.preventDefault();
      document.getElementById('artifact-search')?.focus();
    }
  }

  function handleSearch() {
    if (!searchQuery) {
      searchMatches = [];
      currentMatch = 0;
      return;
    }

    const matches: number[] = [];
    const query = searchQuery.toLowerCase();
    lines.forEach((line, idx) => {
      if (line.toLowerCase().includes(query)) {
        matches.push(idx);
      }
    });
    searchMatches = matches;
    currentMatch = matches.length > 0 ? 0 : -1;

    if (matches.length > 0) {
      scrollToMatch(0);
    }
  }

  function scrollToMatch(idx: number) {
    if (idx >= 0 && idx < searchMatches.length) {
      currentMatch = idx;
      const lineNumber = searchMatches[idx] + 1;
      const lineEl = document.getElementById(`line-${lineNumber}`);
      lineEl?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }

  function nextMatch() {
    if (searchMatches.length > 0) {
      scrollToMatch((currentMatch + 1) % searchMatches.length);
    }
  }

  function prevMatch() {
    if (searchMatches.length > 0) {
      scrollToMatch((currentMatch - 1 + searchMatches.length) % searchMatches.length);
    }
  }

  // Focus trap for accessibility
  onMount(() => {
    if (isOpen) {
      document.body.style.overflow = 'hidden';
    }
    return () => {
      document.body.style.overflow = '';
    };
  });

  $effect(() => {
    if (isOpen) {
      document.body.style.overflow = 'hidden';
    } else {
      document.body.style.overflow = '';
    }
  });
</script>

{#if isOpen}
  <!-- Backdrop -->
  <div
    class="fixed inset-0 bg-black/80 z-50"
    onclick={onClose}
    onkeydown={handleKeydown}
    role="button"
    tabindex="-1"
    aria-label="Close viewer"
  ></div>

  <!-- Slide-out panel -->
  <div
    class="fixed inset-y-0 right-0 w-full max-w-4xl bg-[var(--term-bg)] z-50 flex flex-col shadow-2xl"
    onkeydown={handleKeydown}
    role="dialog"
    aria-modal="true"
    aria-label="Artifact viewer"
    tabindex="-1"
  >
    <!-- Header -->
    <div class="flex items-center justify-between px-4 py-3 bg-[var(--term-bg-secondary)] border-b border-[var(--term-border)]">
      <div class="flex items-center gap-3">
        <button
          onclick={onClose}
          aria-label="Close viewer"
          class="text-[var(--term-text-dim)] hover:text-[var(--term-text)] transition-colors"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
        <div>
          <span class="text-[var(--term-accent)] font-mono text-sm">{displayLang}</span>
          {#if filename}
            <span class="text-[var(--term-text)] font-mono text-sm ml-2">{filename}</span>
          {/if}
          <span class="text-[var(--term-text-dim)] text-xs ml-2">({lineCount} lines)</span>
        </div>
      </div>

      <div class="flex items-center gap-2">
        <!-- Search -->
        <div class="flex items-center gap-1 bg-[var(--term-bg)] rounded px-2 py-1">
          <input
            id="artifact-search"
            type="text"
            bind:value={searchQuery}
            oninput={handleSearch}
            placeholder="Search..."
            aria-label="Search in code"
            class="bg-transparent text-[var(--term-text)] text-sm w-32 outline-none placeholder:text-[var(--term-text-dim)]"
          />
          {#if searchMatches.length > 0}
            <span class="text-[var(--term-text-dim)] text-xs">
              {currentMatch + 1}/{searchMatches.length}
            </span>
            <button onclick={prevMatch} aria-label="Previous match" class="text-[var(--term-text-dim)] hover:text-[var(--term-text)]">
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
              </svg>
            </button>
            <button onclick={nextMatch} aria-label="Next match" class="text-[var(--term-text-dim)] hover:text-[var(--term-text)]">
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
              </svg>
            </button>
          {/if}
        </div>

        <!-- Copy button -->
        <button
          onclick={copyCode}
          aria-label={copied ? 'Code copied' : 'Copy code'}
          class="px-3 py-1 text-sm rounded bg-[var(--term-bg)] text-[var(--term-text-dim)] hover:text-[var(--term-text)] transition-colors"
        >
          {copied ? 'Copied!' : 'Copy'}
        </button>
      </div>
    </div>

    <!-- Code content with line numbers -->
    <div class="flex-1 overflow-auto">
      <pre bind:this={codeEl} class="text-sm font-mono p-4"><code>{#each lines as line, idx}
<span id="line-{idx + 1}" class="flex {searchMatches.includes(idx) ? 'bg-[var(--term-warning)]/20' : ''} {searchMatches[currentMatch] === idx ? 'bg-[var(--term-accent)]/30' : ''}"><span class="select-none text-[var(--term-text-dim)] w-12 text-right pr-4 shrink-0">{idx + 1}</span><span class="text-[var(--term-text)]">{line || ' '}</span></span>{/each}</code></pre>
    </div>

    <!-- Footer with keyboard hints -->
    <div class="px-4 py-2 bg-[var(--term-bg-secondary)] border-t border-[var(--term-border)] text-[var(--term-text-dim)] text-xs font-mono flex gap-4">
      <span><kbd class="px-1 py-0.5 bg-[var(--term-bg)] rounded">Esc</kbd> Close</span>
      <span><kbd class="px-1 py-0.5 bg-[var(--term-bg)] rounded">Cmd/Ctrl+F</kbd> Search</span>
    </div>
  </div>
{/if}

<style>
  pre {
    margin: 0;
    line-height: 1.5;
  }

  code {
    display: block;
  }

  kbd {
    font-family: inherit;
  }
</style>
