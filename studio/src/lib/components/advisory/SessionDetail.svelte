<script lang="ts">
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';
  import type { AdvisorySessionDetail, AdvisoryMessage, ProviderUsage } from '$lib/types/advisory';
  import { formatCost, formatTokens, formatTimestamp } from '$lib/types/advisory';

  interface Props {
    session: AdvisorySessionDetail;
  }

  let { session }: Props = $props();

  // Configure marked
  marked.setOptions({ breaks: true, gfm: true });

  // DOMPurify config
  const PURIFY_CONFIG = {
    ALLOWED_TAGS: [
      'p', 'br', 'strong', 'em', 'b', 'i', 'u', 's', 'del',
      'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      'ul', 'ol', 'li',
      'blockquote', 'pre', 'code',
      'a', 'hr',
    ],
    ALLOWED_ATTR: ['href', 'target', 'rel', 'class'],
  };

  function renderMarkdown(content: string): string {
    try {
      const html = marked.parse(content) as string;
      return DOMPurify.sanitize(html, PURIFY_CONFIG);
    } catch {
      return DOMPurify.sanitize(content);
    }
  }

  function getProviderColor(provider: string | null): string {
    if (!provider) return 'var(--term-text-dim)';

    // Check model_metadata first
    if (session.model_metadata) {
      for (const [key, meta] of Object.entries(session.model_metadata)) {
        if (provider.toLowerCase().includes(key.toLowerCase())) {
          return meta.color;
        }
      }
    }

    // Fallback colors
    if (provider.includes('openai') || provider.includes('gpt')) return '#10a37f';
    if (provider.includes('anthropic') || provider.includes('claude')) return '#d4a574';
    if (provider.includes('gemini') || provider.includes('google')) return '#4285f4';
    if (provider.includes('deepseek')) return '#5c6bc0';
    return 'var(--term-accent)';
  }

  function getProviderName(provider: string | null): string {
    if (!provider) return 'System';

    // Check model_metadata first
    if (session.model_metadata) {
      for (const [key, meta] of Object.entries(session.model_metadata)) {
        if (provider.toLowerCase().includes(key.toLowerCase())) {
          return meta.short_name || meta.display_name;
        }
      }
    }

    // Fallback names
    if (provider.includes('gpt-5')) return 'GPT-5.2';
    if (provider.includes('opus')) return 'Opus 4.5';
    if (provider.includes('gemini-3')) return 'Gemini 3';
    if (provider.includes('deepseek')) return 'DeepSeek';
    return provider;
  }

  // Group messages by turn for council responses
  const groupedMessages = $derived.by(() => {
    const groups: { turn: number; messages: AdvisoryMessage[] }[] = [];
    let currentTurn = -1;

    for (const msg of session.messages) {
      if (msg.turn !== currentTurn) {
        groups.push({ turn: msg.turn, messages: [msg] });
        currentTurn = msg.turn;
      } else {
        groups[groups.length - 1].messages.push(msg);
      }
    }

    return groups;
  });

  // Expand/collapse state per turn
  let expandedTurns = $state<Set<number>>(new Set([0])); // First turn expanded by default

  function toggleTurn(turn: number) {
    const newSet = new Set(expandedTurns);
    if (newSet.has(turn)) {
      newSet.delete(turn);
    } else {
      newSet.add(turn);
    }
    expandedTurns = newSet;
  }
</script>

<div class="session-detail">
  <!-- Session Info -->
  <div class="session-header">
    <div class="session-topic">{session.session.topic || 'Untitled session'}</div>
    <div class="session-meta">
      <span class="meta-item">
        <span class="meta-label">Created:</span>
        <span class="meta-value">{formatTimestamp(session.session.created_at)}</span>
      </span>
      <span class="meta-item">
        <span class="meta-label">Mode:</span>
        <span class="meta-value">{session.session.mode}</span>
      </span>
      <span class="meta-item">
        <span class="meta-label">Status:</span>
        <span class="meta-value">{session.session.status}</span>
      </span>
      {#if session.duration_seconds}
        <span class="meta-item">
          <span class="meta-label">Duration:</span>
          <span class="meta-value">{Math.round(session.duration_seconds)}s</span>
        </span>
      {/if}
    </div>
  </div>

  <!-- Usage Summary -->
  {#if session.usage_by_provider && session.usage_by_provider.length > 0}
    <div class="usage-section">
      <div class="section-title">
        <span>Token Usage</span>
        <span class="total-cost">{formatCost(session.total_cost_usd)}</span>
      </div>
      <div class="usage-grid">
        {#each session.usage_by_provider as pu}
          <div class="usage-card" style="border-left-color: {getProviderColor(pu.provider)}">
            <div class="usage-header">
              <span class="provider-badge" style="background: {getProviderColor(pu.provider)}">
                {getProviderName(pu.provider)}
              </span>
              <span class="usage-cost">{formatCost(pu.cost_usd)}</span>
            </div>
            <div class="usage-details">
              <div class="usage-row">
                <span>Input</span>
                <span>{formatTokens(pu.usage.input_tokens)}</span>
              </div>
              <div class="usage-row">
                <span>Output</span>
                <span>{formatTokens(pu.usage.output_tokens)}</span>
              </div>
              {#if pu.usage.cache_read_tokens > 0}
                <div class="usage-row cache">
                  <span>Cache Read</span>
                  <span>{formatTokens(pu.usage.cache_read_tokens)}</span>
                </div>
              {/if}
              {#if pu.usage.reasoning_tokens > 0}
                <div class="usage-row">
                  <span>Reasoning</span>
                  <span>{formatTokens(pu.usage.reasoning_tokens)}</span>
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Deliberation Result (for council) -->
  {#if session.deliberation_result}
    <div class="result-section">
      <div class="section-title">Final Responses</div>
      <div class="result-cards">
        {#each Object.entries(session.deliberation_result) as [provider, response]}
          <div class="result-card" style="border-left-color: {getProviderColor(provider)}">
            <div class="result-header">
              <span class="provider-badge" style="background: {getProviderColor(provider)}">
                {getProviderName(provider)}
              </span>
            </div>
            <div class="result-content prose">
              {@html renderMarkdown(response)}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Messages -->
  <div class="messages-section">
    <div class="section-title">Conversation</div>
    <div class="messages-list">
      {#each groupedMessages as group (group.turn)}
        <div class="turn-group">
          <button class="turn-header" onclick={() => toggleTurn(group.turn)}>
            <span class="turn-toggle">{expandedTurns.has(group.turn) ? '▼' : '▶'}</span>
            <span class="turn-label">Turn {group.turn + 1}</span>
            <span class="turn-count">{group.messages.length} {group.messages.length === 1 ? 'message' : 'messages'}</span>
          </button>

          {#if expandedTurns.has(group.turn)}
            <div class="turn-messages">
              {#each group.messages as msg}
                <div class="message" class:user={msg.role === 'user'} class:assistant={msg.role === 'assistant'}>
                  <div class="message-header">
                    {#if msg.provider}
                      <span class="provider-badge small" style="background: {getProviderColor(msg.provider)}">
                        {getProviderName(msg.provider)}
                      </span>
                    {:else}
                      <span class="role-badge">{msg.role}</span>
                    {/if}
                    {#if msg.cost_usd}
                      <span class="message-cost">{formatCost(msg.cost_usd)}</span>
                    {/if}
                  </div>
                  <div class="message-content prose">
                    {@html renderMarkdown(msg.content)}
                  </div>
                  {#if msg.usage}
                    <div class="message-usage">
                      <span>in: {formatTokens(msg.usage.input_tokens)}</span>
                      <span>out: {formatTokens(msg.usage.output_tokens)}</span>
                      {#if msg.usage.cache_read_tokens > 0}
                        <span class="cache-hit">cache: {formatTokens(msg.usage.cache_read_tokens)}</span>
                      {/if}
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  </div>

  <!-- Decisions -->
  {#if session.decisions.length > 0}
    <div class="decisions-section">
      <div class="section-title">Decisions</div>
      {#each session.decisions as decision}
        <div class="decision-card">
          <div class="decision-type">{decision.type}</div>
          <div class="decision-topic">{decision.topic}</div>
          {#if decision.rationale}
            <div class="decision-rationale">{decision.rationale}</div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .session-detail {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 16px;
    overflow-y: auto;
    height: 100%;
  }

  .session-header {
    padding-bottom: 12px;
    border-bottom: 1px solid var(--term-border);
  }

  .session-topic {
    font-size: 15px;
    font-weight: 600;
    color: var(--term-text);
    margin-bottom: 8px;
  }

  .session-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    font-size: 12px;
  }

  .meta-item {
    display: flex;
    gap: 4px;
  }

  .meta-label {
    color: var(--term-text-dim);
  }

  .meta-value {
    color: var(--term-text);
    font-weight: 500;
  }

  /* Sections */
  .section-title {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--term-text-dim);
    margin-bottom: 8px;
  }

  .total-cost {
    font-family: var(--font-mono);
    color: var(--term-accent);
  }

  /* Usage Section */
  .usage-grid {
    display: grid;
    gap: 8px;
  }

  .usage-card {
    padding: 10px 12px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-left-width: 3px;
    border-radius: 4px;
  }

  .usage-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 8px;
  }

  .provider-badge {
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 600;
    color: white;
    border-radius: 4px;
  }

  .provider-badge.small {
    padding: 1px 6px;
    font-size: 10px;
  }

  .usage-cost {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--term-accent);
  }

  .usage-details {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .usage-row {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--term-text-dim);
  }

  .usage-row.cache {
    color: var(--term-success);
  }

  /* Result Section */
  .result-cards {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .result-card {
    padding: 12px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-left-width: 3px;
    border-radius: 4px;
  }

  .result-header {
    margin-bottom: 8px;
  }

  .result-content {
    font-size: 13px;
    line-height: 1.5;
    color: var(--term-text);
  }

  /* Messages Section */
  .messages-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .turn-group {
    border: 1px solid var(--term-border);
    border-radius: 4px;
    overflow: hidden;
  }

  .turn-header {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    background: var(--term-bg);
    border: none;
    cursor: pointer;
    font-size: 12px;
    color: var(--term-text);
    text-align: left;
  }

  .turn-header:hover {
    background: var(--term-bg-secondary);
  }

  .turn-toggle {
    font-size: 10px;
    color: var(--term-text-dim);
  }

  .turn-label {
    font-weight: 500;
  }

  .turn-count {
    color: var(--term-text-dim);
    margin-left: auto;
  }

  .turn-messages {
    display: flex;
    flex-direction: column;
    gap: 1px;
    background: var(--term-border);
  }

  .message {
    padding: 10px 12px;
    background: var(--term-bg-secondary);
  }

  .message.user {
    background: var(--term-bg);
  }

  .message-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }

  .role-badge {
    font-size: 10px;
    font-weight: 500;
    text-transform: uppercase;
    color: var(--term-text-dim);
  }

  .message-cost {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--term-accent);
    margin-left: auto;
  }

  .message-content {
    font-size: 13px;
    line-height: 1.5;
    color: var(--term-text);
  }

  .message-usage {
    display: flex;
    gap: 12px;
    margin-top: 6px;
    padding-top: 6px;
    border-top: 1px solid var(--term-border);
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--term-text-dim);
  }

  .cache-hit {
    color: var(--term-success);
  }

  /* Decisions Section */
  .decision-card {
    padding: 10px 12px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    margin-bottom: 8px;
  }

  .decision-type {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--term-accent);
    margin-bottom: 4px;
  }

  .decision-topic {
    font-size: 13px;
    font-weight: 500;
    color: var(--term-text);
  }

  .decision-rationale {
    font-size: 12px;
    color: var(--term-text-dim);
    margin-top: 4px;
  }

  /* Prose styles */
  .prose :global(p) {
    margin: 0.5em 0;
  }

  .prose :global(p:first-child) {
    margin-top: 0;
  }

  .prose :global(p:last-child) {
    margin-bottom: 0;
  }

  .prose :global(code) {
    background: var(--term-bg);
    padding: 0.1em 0.3em;
    border-radius: 3px;
    font-size: 0.9em;
  }

  .prose :global(pre) {
    background: var(--term-bg);
    padding: 0.75em;
    border-radius: 4px;
    overflow-x: auto;
    margin: 0.5em 0;
  }

  .prose :global(pre code) {
    background: none;
    padding: 0;
  }

  .prose :global(strong) {
    font-weight: 600;
  }

  .prose :global(ul), .prose :global(ol) {
    margin: 0.5em 0;
    padding-left: 1.5em;
  }
</style>
