<script lang="ts">
  /**
   * BlockRenderer - Renders message blocks by type
   *
   * Switches on block.type to render the appropriate component.
   * No parsing needed - backend sends structured blocks.
   */

  import type { MessageBlock } from '$lib/api/client';
  import type { CouncilResponses } from '$lib/types/content';
  import TextRenderer from './TextRenderer.svelte';
  import ToolCallInline from './ToolCallInline.svelte';
  import { CodeBlock, CouncilView } from '$lib/components/content';

  interface Props {
    block: MessageBlock;
    blockId: string;
    isStreaming?: boolean;
  }

  let { block, blockId, isStreaming = false }: Props = $props();

  // Convert MessageBlock council fields to CouncilResponses format
  function toCouncilResponses(b: MessageBlock): CouncilResponses {
    return {
      'gpt-5.2': b.gpt,
      'opus-4.5': b.opus,
      'gemini-3-pro': b.gemini,
    };
  }

  // Check if tool call is still loading
  function isToolLoading(b: MessageBlock): boolean {
    return b.type === 'tool_call' && !b.result;
  }
</script>

{#if block.type === 'text'}
  <TextRenderer text={block.content || ''} />
{:else if block.type === 'code_block'}
  <CodeBlock
    id={blockId}
    language={block.language || ''}
    code={block.code || ''}
    filename={block.filename}
  />
{:else if block.type === 'council'}
  <CouncilView
    id={blockId}
    responses={toCouncilResponses(block)}
  />
{:else if block.type === 'tool_call'}
  <ToolCallInline
    callId={block.call_id || blockId}
    name={block.name || 'unknown'}
    arguments={block.arguments || {}}
    summary={block.summary}
    category={block.category}
    result={block.result}
    isLoading={isToolLoading(block)}
  />
{/if}
