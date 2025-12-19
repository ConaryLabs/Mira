<script lang="ts">
  import type { ParsedContent } from '$lib/types/content';
  import CodeBlock from './CodeBlock.svelte';
  import CouncilView from './CouncilView.svelte';
  import ErrorBlock from './ErrorBlock.svelte';
  import WarningBlock from './WarningBlock.svelte';
  import TextBlock from './TextBlock.svelte';

  interface Props {
    content: ParsedContent;
  }

  let { content }: Props = $props();
</script>

{#if content.type === 'code_block'}
  <CodeBlock
    id={content.id}
    language={content.language}
    code={content.code}
    filename={content.filename}
  />
{:else if content.type === 'council'}
  <CouncilView id={content.id} responses={content.responses} />
{:else if content.type === 'error'}
  <ErrorBlock message={content.message} code={content.code} />
{:else if content.type === 'warning'}
  <WarningBlock message={content.message} />
{:else if content.type === 'text'}
  <TextBlock content={content.content} />
{:else if content.type === 'diff'}
  <!-- Diff is handled separately via DiffView, but we can add support here if needed -->
  <div class="text-[var(--term-text-dim)] text-sm">
    [Diff: {content.path}]
  </div>
{/if}
