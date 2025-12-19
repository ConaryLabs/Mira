/**
 * DebouncedParser - Debounces parsing during rapid streaming
 *
 * Problem: During streaming, content changes every ~50ms. Parsing on every
 * change causes CPU spikes and UI jitter.
 *
 * Solution: Only parse when content has been stable for DEBOUNCE_MS.
 * During rapid updates, show simplified/raw content.
 */

import { parseTextContent, type ParseResult } from './contentParser';
import type { ParsedContent } from '$lib/types/content';

const DEBOUNCE_MS = 150; // Wait this long after last change before parsing

interface CachedParse {
  content: string;
  result: ParseResult;
  timestamp: number;
}

/**
 * Creates a debounced parser instance for a streaming block
 * Uses Svelte 5 runes for reactivity
 */
export class DebouncedParser {
  private cache: CachedParse | null = null;
  private pendingTimeout: ReturnType<typeof setTimeout> | null = null;
  private idPrefix: string;

  // Reactive state - triggers UI update when changed
  result = $state<ParseResult>({
    segments: [],
    metadata: { hasCode: false, hasErrors: false, hasWarnings: false, isCouncil: false },
  });

  // Track if we're showing stale/simplified content
  isPending = $state(false);

  constructor(idPrefix: string = 'streaming') {
    this.idPrefix = idPrefix;
  }

  /**
   * Update with new content - debounces the actual parsing
   */
  update(content: string): void {
    if (!content || content.trim() === '') {
      this.result = {
        segments: [],
        metadata: { hasCode: false, hasErrors: false, hasWarnings: false, isCouncil: false },
      };
      this.isPending = false;
      return;
    }

    // If content hasn't changed, do nothing
    if (this.cache?.content === content) {
      return;
    }

    // Clear any pending parse
    if (this.pendingTimeout) {
      clearTimeout(this.pendingTimeout);
    }

    // Check if we can use cached result (content only appended)
    if (this.cache && content.startsWith(this.cache.content)) {
      // Content was appended - we can show cached result while waiting
      // This prevents "flash" where content disappears during debounce
    }

    // Mark as pending (we have new content but haven't parsed yet)
    this.isPending = true;

    // If no cached result yet, do immediate simplified parse
    if (!this.cache) {
      this.result = this.simplifiedParse(content);
    }

    // Schedule debounced full parse
    this.pendingTimeout = setTimeout(() => {
      this.fullParse(content);
    }, DEBOUNCE_MS);
  }

  /**
   * Simplified parse - fast, used during rapid updates
   * Only extracts obvious structure, no expensive operations
   */
  private simplifiedParse(content: string): ParseResult {
    const segments: ParsedContent[] = [];

    // Quick check for code block start (don't wait for it to close)
    const codeBlockStart = content.match(/```(\w+)?\n/);
    if (codeBlockStart) {
      const startIdx = codeBlockStart.index!;
      const beforeCode = content.slice(0, startIdx).trim();
      const codeContent = content.slice(startIdx + codeBlockStart[0].length);

      // Check if code block is closed
      const closeIdx = codeContent.indexOf('```');

      if (beforeCode) {
        segments.push({
          type: 'text',
          id: `${this.idPrefix}-pre`,
          content: beforeCode,
        });
      }

      if (closeIdx >= 0) {
        // Closed code block
        segments.push({
          type: 'code_block',
          id: `${this.idPrefix}-code`,
          language: codeBlockStart[1] || 'text',
          code: codeContent.slice(0, closeIdx).trim(),
        });
        // Content after code block
        const afterCode = codeContent.slice(closeIdx + 3).trim();
        if (afterCode) {
          segments.push({
            type: 'text',
            id: `${this.idPrefix}-post`,
            content: afterCode,
          });
        }
      } else {
        // Unclosed code block (still streaming)
        segments.push({
          type: 'code_block',
          id: `${this.idPrefix}-code`,
          language: codeBlockStart[1] || 'text',
          code: codeContent,
        });
      }
    } else {
      // Plain text
      segments.push({
        type: 'text',
        id: `${this.idPrefix}-text`,
        content: content,
      });
    }

    return {
      segments,
      metadata: {
        hasCode: segments.some(s => s.type === 'code_block'),
        hasErrors: false,
        hasWarnings: false,
        isCouncil: false,
      },
    };
  }

  /**
   * Full parse - expensive, only called after debounce
   */
  private fullParse(content: string): void {
    const result = parseTextContent(content, this.idPrefix, true);

    this.cache = {
      content,
      result,
      timestamp: Date.now(),
    };

    this.result = result;
    this.isPending = false;
    this.pendingTimeout = null;
  }

  /**
   * Force immediate parse (e.g., when streaming ends)
   */
  finalize(content: string): ParseResult {
    if (this.pendingTimeout) {
      clearTimeout(this.pendingTimeout);
      this.pendingTimeout = null;
    }

    // Use finalized parsing (not streaming mode) for final result
    const result = parseTextContent(content, this.idPrefix, false);

    this.cache = {
      content,
      result,
      timestamp: Date.now(),
    };

    this.result = result;
    this.isPending = false;

    return result;
  }

  /**
   * Clean up when component unmounts
   */
  destroy(): void {
    if (this.pendingTimeout) {
      clearTimeout(this.pendingTimeout);
      this.pendingTimeout = null;
    }
  }
}

/**
 * Factory function for creating parser instances per block
 */
const parserInstances = new Map<string, DebouncedParser>();

export function getParser(blockId: string): DebouncedParser {
  let parser = parserInstances.get(blockId);
  if (!parser) {
    parser = new DebouncedParser(blockId);
    parserInstances.set(blockId, parser);
  }
  return parser;
}

export function removeParser(blockId: string): void {
  const parser = parserInstances.get(blockId);
  if (parser) {
    parser.destroy();
    parserInstances.delete(blockId);
  }
}

export function clearAllParsers(): void {
  parserInstances.forEach(parser => parser.destroy());
  parserInstances.clear();
}
