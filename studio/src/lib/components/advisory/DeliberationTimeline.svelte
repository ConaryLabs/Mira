<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';
  import type { CouncilProgressEvent, TimelineEvent } from '$lib/types/advisory';
  import { getModelInfo, formatDuration } from '$lib/types/advisory';

  interface Props {
    message: string;
    projectId?: number;
    onComplete?: (sessionId: string) => void;
    onError?: (error: string) => void;
  }

  let { message, projectId, onComplete, onError }: Props = $props();

  // State
  let status = $state<'idle' | 'connecting' | 'streaming' | 'complete' | 'error'>('idle');
  let sessionId = $state<string | null>(null);
  let currentRound = $state(0);
  let maxRounds = $state(3);
  let events = $state<TimelineEvent[]>([]);
  let modelResponses = $state<Record<string, string>>({});
  let activeModels = $state<Set<string>>(new Set());
  let completedModels = $state<Set<string>>(new Set());
  let expandedModels = $state<Set<string>>(new Set());
  let synthesis = $state('');
  let errorMessage = $state<string | null>(null);
  let startTime = $state<Date | null>(null);
  let elapsedSeconds = $state(0);

  // Auto-scroll control
  let timelineEl: HTMLDivElement;
  let autoScroll = $state(true);

  // Timer for elapsed time
  let timerInterval: number | undefined;

  // EventSource for SSE
  let eventSource: EventSource | null = null;

  // Configure marked
  marked.setOptions({ breaks: true, gfm: true });
  const PURIFY_CONFIG = {
    ALLOWED_TAGS: ['p', 'br', 'strong', 'em', 'code', 'pre', 'ul', 'ol', 'li', 'blockquote'],
    ALLOWED_ATTR: ['class'],
  };

  function renderMarkdown(content: string): string {
    try {
      const html = marked.parse(content) as string;
      return DOMPurify.sanitize(html, PURIFY_CONFIG);
    } catch {
      return DOMPurify.sanitize(content);
    }
  }

  function addEvent(type: string, model?: string, content?: string, metadata?: Record<string, unknown>) {
    const event: TimelineEvent = {
      id: events.length,
      type,
      timestamp: new Date(),
      model,
      content,
      metadata,
    };
    events = [...events, event];

    // Auto-scroll to bottom
    if (autoScroll && timelineEl) {
      requestAnimationFrame(() => {
        timelineEl.scrollTop = timelineEl.scrollHeight;
      });
    }
  }

  function handleEvent(event: CouncilProgressEvent) {
    switch (event.type) {
      case 'session_created':
        sessionId = event.session_id;
        addEvent('session_created', undefined, `Session ${event.session_id.slice(0, 8)}`);
        break;

      case 'round_started':
        currentRound = event.round;
        maxRounds = event.max_rounds;
        addEvent('round_started', undefined, undefined, { round: event.round, maxRounds: event.max_rounds });
        break;

      case 'model_started':
        activeModels = new Set([...activeModels, event.model]);
        modelResponses = { ...modelResponses, [event.model]: '' };
        addEvent('model_started', event.model);
        break;

      case 'model_delta':
        modelResponses = {
          ...modelResponses,
          [event.model]: (modelResponses[event.model] || '') + event.delta,
        };
        break;

      case 'model_completed':
        activeModels = new Set([...activeModels].filter(m => m !== event.model));
        completedModels = new Set([...completedModels, event.model]);
        modelResponses = { ...modelResponses, [event.model]: event.text };
        addEvent('model_completed', event.model, event.text);
        break;

      case 'model_timeout':
        activeModels = new Set([...activeModels].filter(m => m !== event.model));
        addEvent('model_timeout', event.model);
        break;

      case 'model_error':
        activeModels = new Set([...activeModels].filter(m => m !== event.model));
        addEvent('model_error', event.model, event.error);
        break;

      case 'moderator_analyzing':
        addEvent('moderator_analyzing', undefined, undefined, { round: event.round });
        break;

      case 'moderator_complete':
        addEvent('moderator_complete', undefined, undefined, {
          round: event.round,
          shouldContinue: event.should_continue,
          disagreements: event.disagreements,
          focusQuestions: event.focus_questions,
          resolvedPoints: event.resolved_points,
        });
        break;

      case 'early_consensus':
        addEvent('early_consensus', undefined, event.reason || 'Consensus reached', { round: event.round });
        break;

      case 'synthesis_started':
        addEvent('synthesis_started');
        break;

      case 'synthesis_delta':
        synthesis += event.delta;
        break;

      case 'done':
      case 'deliberation_complete':
        status = 'complete';
        addEvent('complete', undefined, undefined, { result: event.type === 'done' ? event.result : (event as { result: unknown }).result });
        if (sessionId && onComplete) {
          onComplete(sessionId);
        }
        break;

      case 'deliberation_failed':
        status = 'error';
        errorMessage = event.error;
        addEvent('error', undefined, event.error);
        if (onError) {
          onError(event.error);
        }
        break;
    }
  }

  function toggleModelExpanded(model: string) {
    if (expandedModels.has(model)) {
      expandedModels = new Set([...expandedModels].filter(m => m !== model));
    } else {
      expandedModels = new Set([...expandedModels, model]);
    }
  }

  async function startDeliberation() {
    status = 'connecting';
    startTime = new Date();
    events = [];
    modelResponses = {};
    activeModels = new Set();
    completedModels = new Set();
    expandedModels = new Set();
    synthesis = '';
    errorMessage = null;
    currentRound = 0;

    // Start timer
    timerInterval = window.setInterval(() => {
      if (startTime) {
        elapsedSeconds = (Date.now() - startTime.getTime()) / 1000;
      }
    }, 100);

    addEvent('connecting');

    try {
      // Start SSE connection
      const response = await fetch('/api/advisory/deliberate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          message,
          project_id: projectId,
        }),
      });

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
      }

      if (!response.body) {
        throw new Error('No response body');
      }

      status = 'streaming';
      addEvent('connected');

      // Read SSE stream
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });

        // Parse SSE lines
        const lines = buffer.split('\n');
        buffer = lines.pop() || ''; // Keep incomplete line in buffer

        for (const line of lines) {
          if (line.startsWith('data: ')) {
            const data = line.slice(6);
            if (data) {
              try {
                const event = JSON.parse(data) as CouncilProgressEvent;
                handleEvent(event);
              } catch (e) {
                console.warn('Failed to parse SSE event:', data, e);
              }
            }
          }
        }
      }

      // Stream ended
      if (status === 'streaming') {
        status = 'complete';
      }
    } catch (e) {
      status = 'error';
      errorMessage = e instanceof Error ? e.message : 'Connection failed';
      addEvent('error', undefined, errorMessage);
      if (onError) {
        onError(errorMessage);
      }
    } finally {
      if (timerInterval) {
        clearInterval(timerInterval);
      }
    }
  }

  function handleScroll() {
    if (!timelineEl) return;
    const { scrollTop, scrollHeight, clientHeight } = timelineEl;
    // Auto-scroll if near bottom
    autoScroll = scrollHeight - scrollTop - clientHeight < 50;
  }

  onMount(() => {
    startDeliberation();
  });

  onDestroy(() => {
    if (eventSource) {
      eventSource.close();
    }
    if (timerInterval) {
      clearInterval(timerInterval);
    }
  });
</script>

<div class="deliberation-timeline">
  <!-- Header -->
  <div class="timeline-header">
    <div class="status-section">
      <span class="status-indicator {status}"></span>
      <span class="status-text">
        {#if status === 'connecting'}Connecting...
        {:else if status === 'streaming'}Deliberating
        {:else if status === 'complete'}Complete
        {:else if status === 'error'}Error
        {:else}Ready{/if}
      </span>
      {#if currentRound > 0}
        <span class="round-badge">Round {currentRound}/{maxRounds}</span>
      {/if}
    </div>
    <div class="timer">
      {formatDuration(elapsedSeconds)}
    </div>
  </div>

  <!-- Model Response Cards -->
  {#if Object.keys(modelResponses).length > 0}
    <div class="model-response-cards">
      {#each Object.entries(modelResponses) as [model, text]}
        {@const info = getModelInfo(model)}
        {@const isActive = activeModels.has(model)}
        {@const isComplete = completedModels.has(model)}
        {@const isExpanded = expandedModels.has(model)}
        <div class="model-card" style="--model-color: {info.color}">
          <button
            class="model-card-header"
            onclick={() => toggleModelExpanded(model)}
          >
            <div class="model-card-left">
              <span class="model-badge" style="background: {info.color}">{info.shortName}</span>
              <span class="model-name">{info.name}</span>
              {#if isActive}
                <span class="typing-indicator">
                  <span class="dot"></span>
                  <span class="dot"></span>
                  <span class="dot"></span>
                </span>
              {/if}
            </div>
            <div class="model-card-right">
              <span class="char-count">{text.length} chars</span>
              {#if isComplete}
                <span class="status-badge complete">✓</span>
              {:else if isActive}
                <span class="status-badge streaming">●</span>
              {/if}
              <svg class="expand-icon {isExpanded ? 'expanded' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M6 9l6 6 6-6" />
              </svg>
            </div>
          </button>
          {#if isExpanded || isActive}
            <div class="model-card-content {isExpanded ? 'expanded' : 'preview'}">
              <div class="response-text prose">
                {@html renderMarkdown(text)}
              </div>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <!-- Synthesis in Progress -->
  {#if synthesis.length > 0 && status === 'streaming'}
    <div class="synthesis-live">
      <div class="synthesis-header">
        <svg class="synthesis-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707" />
        </svg>
        <span>Synthesizing...</span>
      </div>
      <div class="synthesis-content prose">
        {@html renderMarkdown(synthesis)}
      </div>
    </div>
  {/if}

  <!-- Timeline Events -->
  <div class="timeline-events" bind:this={timelineEl} onscroll={handleScroll}>
    {#each events as event (event.id)}
      <div class="timeline-event {event.type}">
        <div class="event-time">
          {event.timestamp.toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' })}
        </div>
        <div class="event-content">
          {#if event.type === 'connecting'}
            <span class="event-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="10" />
                <path d="M12 6v6l4 2" />
              </svg>
            </span>
            <span>Connecting to advisory service...</span>

          {:else if event.type === 'connected' || event.type === 'session_created'}
            <span class="event-icon success">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                <path d="M22 4L12 14.01l-3-3" />
              </svg>
            </span>
            <span>{event.content || 'Connected'}</span>

          {:else if event.type === 'round_started'}
            <span class="event-icon round">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="10" />
                <path d="M12 16v-4M12 8h.01" />
              </svg>
            </span>
            <span class="event-title">Round {event.metadata?.round}/{event.metadata?.maxRounds}</span>

          {:else if event.type === 'model_started'}
            {@const info = getModelInfo(event.model || '')}
            <span class="model-badge" style="background: {info.color}">{info.shortName}</span>
            <span>started responding</span>

          {:else if event.type === 'model_completed'}
            {@const info = getModelInfo(event.model || '')}
            <span class="model-badge" style="background: {info.color}">{info.shortName}</span>
            <span>completed ({(event.content?.length || 0)} chars)</span>

          {:else if event.type === 'model_timeout'}
            {@const info = getModelInfo(event.model || '')}
            <span class="model-badge timeout" style="background: {info.color}">{info.shortName}</span>
            <span class="warning">timed out</span>

          {:else if event.type === 'model_error'}
            {@const info = getModelInfo(event.model || '')}
            <span class="model-badge error" style="background: {info.color}">{info.shortName}</span>
            <span class="error-text">{event.content}</span>

          {:else if event.type === 'moderator_analyzing'}
            <span class="event-icon moderator">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z" />
              </svg>
            </span>
            <span>Moderator analyzing responses...</span>

          {:else if event.type === 'moderator_complete'}
            <span class="event-icon moderator">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
            </span>
            <div class="moderator-result">
              <span>{event.metadata?.shouldContinue ? 'Continue to next round' : 'Ready to synthesize'}</span>
              {#if (event.metadata?.disagreements as string[])?.length > 0}
                <div class="moderator-items">
                  <span class="item-label">Disagreements:</span>
                  {#each event.metadata?.disagreements as string[] as d}
                    <span class="item">{d}</span>
                  {/each}
                </div>
              {/if}
              {#if (event.metadata?.resolvedPoints as string[])?.length > 0}
                <div class="moderator-items resolved">
                  <span class="item-label">Resolved:</span>
                  {#each event.metadata?.resolvedPoints as string[] as p}
                    <span class="item">{p}</span>
                  {/each}
                </div>
              {/if}
            </div>

          {:else if event.type === 'early_consensus'}
            <span class="event-icon success">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M14 9V5a3 3 0 0 0-3-3l-4 9v11h11.28a2 2 0 0 0 2-1.7l1.38-9a2 2 0 0 0-2-2.3zM7 22H4a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2h3" />
              </svg>
            </span>
            <span class="success-text">Early consensus reached! {event.content}</span>

          {:else if event.type === 'synthesis_started'}
            <span class="event-icon synthesis">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707" />
              </svg>
            </span>
            <span>Synthesizing final response...</span>

          {:else if event.type === 'complete'}
            <span class="event-icon success">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                <path d="M22 4L12 14.01l-3-3" />
              </svg>
            </span>
            <span class="success-text">Deliberation complete</span>

          {:else if event.type === 'error'}
            <span class="event-icon error">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="10" />
                <path d="M15 9l-6 6M9 9l6 6" />
              </svg>
            </span>
            <span class="error-text">{event.content}</span>

          {:else}
            <span>{event.type}</span>
          {/if}
        </div>
      </div>
    {/each}
  </div>

  <!-- Auto-scroll indicator -->
  {#if !autoScroll && status === 'streaming'}
    <button class="scroll-to-bottom" onclick={() => { autoScroll = true; if (timelineEl) timelineEl.scrollTop = timelineEl.scrollHeight; }}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M19 14l-7 7m0 0l-7-7m7 7V3" />
      </svg>
      Resume auto-scroll
    </button>
  {/if}

  <!-- Final Synthesis -->
  {#if status === 'complete' && synthesis.length > 0}
    <div class="final-synthesis">
      <div class="synthesis-header complete">
        <svg class="synthesis-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
          <path d="M22 4L12 14.01l-3-3" />
        </svg>
        <span>Final Synthesis</span>
      </div>
      <div class="synthesis-content prose">
        {@html renderMarkdown(synthesis)}
      </div>
    </div>
  {/if}

  <!-- Error Display -->
  {#if status === 'error'}
    <div class="error-panel">
      <svg class="error-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="10" />
        <path d="M15 9l-6 6M9 9l6 6" />
      </svg>
      <div class="error-details">
        <span class="error-title">Deliberation Failed</span>
        <span class="error-message">{errorMessage}</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .deliberation-timeline {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--term-bg);
    overflow: hidden;
  }

  /* Header */
  .timeline-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-bg-secondary);
  }

  .status-section {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .status-indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--term-text-dim);
  }

  .status-indicator.connecting {
    background: var(--term-warning);
    animation: pulse 1s infinite;
  }

  .status-indicator.streaming {
    background: var(--term-accent);
    animation: pulse 1s infinite;
  }

  .status-indicator.complete {
    background: var(--term-success);
  }

  .status-indicator.error {
    background: var(--term-error);
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  .status-text {
    font-size: 13px;
    font-weight: 500;
    color: var(--term-text);
  }

  .round-badge {
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 600;
    color: var(--term-accent);
    background: rgba(var(--term-accent-rgb, 100, 149, 237), 0.15);
    border-radius: 4px;
  }

  .timer {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--term-text-dim);
  }

  /* Model Response Cards */
  .model-response-cards {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--term-border);
  }

  .model-card {
    border: 1px solid var(--term-border);
    border-left: 3px solid var(--model-color);
    border-radius: 6px;
    overflow: hidden;
    background: var(--term-bg);
    transition: border-color 0.15s;
  }

  .model-card:hover {
    border-color: var(--model-color);
  }

  .model-card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    padding: 10px 12px;
    background: var(--term-bg-secondary);
    border: none;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s;
  }

  .model-card-header:hover {
    background: var(--term-bg);
  }

  .model-card-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .model-card-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .model-badge {
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 600;
    color: white;
    border-radius: 4px;
  }

  .model-badge.timeout {
    opacity: 0.6;
  }

  .model-badge.error {
    opacity: 0.6;
  }

  .model-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--term-text);
  }

  .typing-indicator {
    display: flex;
    gap: 2px;
  }

  .typing-indicator .dot {
    width: 4px;
    height: 4px;
    background: var(--model-color, var(--term-accent));
    border-radius: 50%;
    animation: typing 1.4s infinite;
  }

  .typing-indicator .dot:nth-child(2) { animation-delay: 0.2s; }
  .typing-indicator .dot:nth-child(3) { animation-delay: 0.4s; }

  @keyframes typing {
    0%, 60%, 100% { transform: translateY(0); }
    30% { transform: translateY(-4px); }
  }

  .char-count {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--term-text-dim);
  }

  .status-badge {
    font-size: 10px;
    font-weight: 600;
  }

  .status-badge.complete {
    color: var(--term-success);
  }

  .status-badge.streaming {
    color: var(--model-color, var(--term-accent));
    animation: pulse 1s infinite;
  }

  .expand-icon {
    width: 16px;
    height: 16px;
    color: var(--term-text-dim);
    transition: transform 0.2s;
  }

  .expand-icon.expanded {
    transform: rotate(180deg);
  }

  .model-card-content {
    border-top: 1px solid var(--term-border);
    overflow: hidden;
    transition: max-height 0.3s ease;
  }

  .model-card-content.preview {
    max-height: 200px;
    overflow-y: auto;
  }

  .model-card-content.expanded {
    max-height: 500px;
    overflow-y: auto;
  }

  .response-text {
    padding: 12px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--term-text);
  }

  /* Synthesis */
  .synthesis-live, .final-synthesis {
    padding: 12px 16px;
    border-bottom: 1px solid var(--term-border);
  }

  .synthesis-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--term-accent);
  }

  .synthesis-header.complete {
    color: var(--term-success);
  }

  .synthesis-icon {
    width: 16px;
    height: 16px;
  }

  .synthesis-content {
    font-size: 13px;
    line-height: 1.5;
    color: var(--term-text);
  }

  .final-synthesis {
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-success);
    border-radius: 4px;
    margin: 12px;
  }

  /* Timeline Events */
  .timeline-events {
    flex: 1;
    overflow-y: auto;
    padding: 8px 16px;
  }

  .timeline-event {
    display: flex;
    gap: 12px;
    padding: 6px 0;
    font-size: 12px;
    border-bottom: 1px solid var(--term-border);
  }

  .timeline-event:last-child {
    border-bottom: none;
  }

  .event-time {
    flex-shrink: 0;
    width: 60px;
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--term-text-dim);
  }

  .event-content {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    flex: 1;
    color: var(--term-text);
  }

  .event-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    color: var(--term-text-dim);
  }

  .event-icon svg {
    width: 14px;
    height: 14px;
  }

  .event-icon.success { color: var(--term-success); }
  .event-icon.error { color: var(--term-error); }
  .event-icon.round { color: var(--term-accent); }
  .event-icon.moderator { color: var(--term-warning); }
  .event-icon.synthesis { color: var(--term-accent); }

  .event-title {
    font-weight: 600;
    color: var(--term-accent);
  }

  .warning {
    color: var(--term-warning);
  }

  .error-text {
    color: var(--term-error);
  }

  .success-text {
    color: var(--term-success);
  }

  .moderator-result {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .moderator-items {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    font-size: 11px;
  }

  .moderator-items.resolved {
    color: var(--term-success);
  }

  .item-label {
    color: var(--term-text-dim);
  }

  .item {
    padding: 1px 6px;
    background: var(--term-bg-secondary);
    border-radius: 3px;
  }

  /* Scroll button */
  .scroll-to-bottom {
    position: absolute;
    bottom: 80px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    font-size: 11px;
    color: white;
    background: var(--term-accent);
    border: none;
    border-radius: 16px;
    cursor: pointer;
    opacity: 0.9;
    transition: opacity 0.15s;
  }

  .scroll-to-bottom:hover {
    opacity: 1;
  }

  .scroll-to-bottom svg {
    width: 14px;
    height: 14px;
  }

  /* Error Panel */
  .error-panel {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 16px;
    margin: 12px;
    background: rgba(var(--term-error-rgb, 220, 53, 69), 0.1);
    border: 1px solid var(--term-error);
    border-radius: 6px;
  }

  .error-panel .error-icon {
    width: 24px;
    height: 24px;
    color: var(--term-error);
    flex-shrink: 0;
  }

  .error-details {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .error-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--term-error);
  }

  .error-message {
    font-size: 12px;
    color: var(--term-text-dim);
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
</style>
