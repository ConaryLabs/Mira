/**
 * Content Parser - Two-pass parsing with streaming support
 *
 * Key improvements from council review:
 * 1. LRU cache with 100 entry limit (prevents memory leaks)
 * 2. Streaming vs finalized parsing modes
 * 3. Partial content handling (unclosed code blocks, partial JSON)
 * 4. FallbackToText on parse failures
 */

import type { ParsedContent, ParseResult, ContentKind, CouncilResponses } from '$lib/types/content';

// ============================================
// LRU CACHE (100 entries max)
// ============================================

class LRUCache<K, V> {
  private cache = new Map<K, V>();
  private maxSize: number;

  constructor(maxSize: number = 100) {
    this.maxSize = maxSize;
  }

  get(key: K): V | undefined {
    const value = this.cache.get(key);
    if (value !== undefined) {
      // Move to end (most recently used)
      this.cache.delete(key);
      this.cache.set(key, value);
    }
    return value;
  }

  set(key: K, value: V): void {
    // Delete first to update position
    if (this.cache.has(key)) {
      this.cache.delete(key);
    }
    this.cache.set(key, value);

    // Evict oldest if over limit
    if (this.cache.size > this.maxSize) {
      const firstKey = this.cache.keys().next().value;
      if (firstKey !== undefined) {
        this.cache.delete(firstKey);
      }
    }
  }

  clear(): void {
    this.cache.clear();
  }

  get size(): number {
    return this.cache.size;
  }
}

// Separate caches for different parsing stages
const parseCache = new LRUCache<string, ParsedContent[]>(100);

// Parser version - increment when parser logic changes to invalidate old cache
const PARSER_VERSION = 'v3';

// ============================================
// FAST HASH (non-crypto, for streaming)
// ============================================

function fastHash(content: string): string {
  // During streaming, use cheap hash: length + first/last chars
  // This avoids O(n) hashing on every token
  if (content.length < 100) {
    // Short content: use full hash
    let hash = 0;
    for (let i = 0; i < content.length; i++) {
      hash = ((hash << 5) - hash) + content.charCodeAt(i);
      hash = hash & hash;
    }
    return hash.toString(36);
  }

  // Long content: sample-based hash
  const first = content.slice(0, 50);
  const last = content.slice(-50);
  const sample = first + last + content.length;
  let hash = 0;
  for (let i = 0; i < sample.length; i++) {
    hash = ((hash << 5) - hash) + sample.charCodeAt(i);
    hash = hash & hash;
  }
  return `s${hash.toString(36)}`;
}

// Generate stable IDs for parsed blocks
let idCounter = 0;
function generateId(prefix: string): string {
  return `${prefix}-${++idCounter}`;
}

export function resetIdCounter(): void {
  idCounter = 0;
}

// ============================================
// PATTERNS
// ============================================

// Closed code block (complete)
const CODE_BLOCK_CLOSED = /```(\w+)?\n([\s\S]*?)```/g;

// Unclosed code block (streaming) - detects start without end
const CODE_BLOCK_UNCLOSED = /```(\w+)?\n([\s\S]*)$/;

// Council JSON pattern
const COUNCIL_PATTERN = /"council"\s*:\s*\{/;

// Error/warning patterns (conservative to avoid false positives)
const ERROR_PATTERNS = [
  /^error(\[E\d+\])?:\s*(.+)$/im,
  /^Error:\s*(.+)$/m,
  /^ERROR:\s*(.+)$/m,
  /^fatal:\s*(.+)$/im,
  /^panic:\s*(.+)$/im,
];

const WARNING_PATTERNS = [
  /^warning(\[\w+\])?:\s*(.+)$/im,
  /^Warning:\s*(.+)$/m,
  /^WARN(ING)?:\s*(.+)$/m,
];

// ============================================
// PASS 1: Classification (fast, regex-based)
// ============================================

export function classify(text: string): ContentKind[] {
  const kinds: ContentKind[] = [];

  // Check for closed code blocks
  CODE_BLOCK_CLOSED.lastIndex = 0;
  if (CODE_BLOCK_CLOSED.test(text)) {
    kinds.push('code_block');
  }

  if (COUNCIL_PATTERN.test(text)) {
    kinds.push('council');
  }

  for (const pattern of ERROR_PATTERNS) {
    pattern.lastIndex = 0;
    if (pattern.test(text)) {
      kinds.push('error');
      break;
    }
  }

  for (const pattern of WARNING_PATTERNS) {
    pattern.lastIndex = 0;
    if (pattern.test(text)) {
      kinds.push('warning');
      break;
    }
  }

  if (kinds.length === 0) {
    kinds.push('text');
  }

  return kinds;
}

// ============================================
// COUNCIL PARSING
// ============================================

// Valid provider keys we expect in council responses
const VALID_PROVIDER_KEYS = new Set(['openai', 'deepseek', 'gemini', 'gpt-5.2']);

// Pattern to detect council JSON start
const COUNCIL_START_PATTERN = /^\s*\{\s*"council"\s*:/;

/**
 * Check if text is incomplete council JSON (for streaming)
 * Returns true if it looks like council JSON is being streamed but not complete
 */
export function isIncompleteCouncilJson(text: string): boolean {
  const trimmed = text.trim();

  // Must start like council JSON
  if (!COUNCIL_START_PATTERN.test(trimmed)) {
    return false;
  }

  // If it ends with } or ], try parsing - if it fails, it's incomplete
  if (trimmed.endsWith('}') || trimmed.endsWith(']')) {
    try {
      JSON.parse(trimmed);
      return false; // Valid JSON, not incomplete
    } catch {
      return true; // Ends with } but invalid - incomplete
    }
  }

  // Doesn't end with closing brace - definitely incomplete
  return true;
}

/**
 * Check if text looks like a council response
 * Uses try-catch JSON.parse with provider key validation to avoid false positives
 */
export function isCouncilResponse(text: string): boolean {
  // Quick regex check first (cheap)
  if (!COUNCIL_PATTERN.test(text)) {
    return false;
  }

  // Try actual JSON parse to validate (more reliable than regex)
  try {
    const parsed = JSON.parse(text);
    if (parsed.council && typeof parsed.council === 'object') {
      // Verify it has at least one valid provider key
      const keys = Object.keys(parsed.council);
      return keys.some(k => VALID_PROVIDER_KEYS.has(k.toLowerCase()));
    }
  } catch {
    // Not valid JSON at top level, but might have council embedded
    // Check if it looks like council JSON structure
    const hasProviderKey = Array.from(VALID_PROVIDER_KEYS).some(
      provider => text.includes(`"${provider}"`) || text.includes(`"${provider.toUpperCase()}"`)
    );
    return hasProviderKey && COUNCIL_PATTERN.test(text);
  }

  return false;
}

/**
 * Parse council response from JSON text
 * Handles both complete and partial JSON
 */
export function parseCouncilResponse(text: string): CouncilResponses | null {
  // Try parsing as complete JSON first
  try {
    const parsed = JSON.parse(text);
    if (parsed.council && typeof parsed.council === 'object') {
      // Validate that it has at least one valid provider
      const council = parsed.council as Record<string, string>;
      const validKeys = Object.keys(council).filter(k =>
        VALID_PROVIDER_KEYS.has(k.toLowerCase())
      );
      if (validKeys.length > 0) {
        // Normalize keys to lowercase
        const normalized: CouncilResponses = {};
        for (const [key, value] of Object.entries(council)) {
          const normalizedKey = key.toLowerCase() === 'gpt-5.2' ? 'openai' : key.toLowerCase();
          if (VALID_PROVIDER_KEYS.has(key.toLowerCase()) && typeof value === 'string') {
            normalized[normalizedKey as keyof CouncilResponses] = value;
          }
        }
        return normalized;
      }
    }
  } catch {
    // Try to extract council object from embedded JSON
  }

  // Try to extract council from partial/embedded JSON
  try {
    const match = text.match(/"council"\s*:\s*(\{[\s\S]*)/);
    if (match) {
      const councilStr = match[1];
      // Balance braces to find complete object
      let depth = 0;
      let end = 0;
      let inString = false;
      let escape = false;

      for (let i = 0; i < councilStr.length; i++) {
        const char = councilStr[i];

        if (escape) {
          escape = false;
          continue;
        }

        if (char === '\\') {
          escape = true;
          continue;
        }

        if (char === '"' && !escape) {
          inString = !inString;
          continue;
        }

        if (!inString) {
          if (char === '{') depth++;
          else if (char === '}') {
            depth--;
            if (depth === 0) {
              end = i + 1;
              break;
            }
          }
        }
      }

      if (end > 0) {
        const council = JSON.parse(councilStr.slice(0, end));
        // Validate and normalize
        const validKeys = Object.keys(council).filter(k =>
          VALID_PROVIDER_KEYS.has(k.toLowerCase())
        );
        if (validKeys.length > 0) {
          const normalized: CouncilResponses = {};
          for (const [key, value] of Object.entries(council)) {
            const normalizedKey = key.toLowerCase() === 'gpt-5.2' ? 'openai' : key.toLowerCase();
            if (VALID_PROVIDER_KEYS.has(key.toLowerCase()) && typeof value === 'string') {
              normalized[normalizedKey as keyof CouncilResponses] = value;
            }
          }
          return normalized;
        }
      }
    }
  } catch {
    // Fall through
  }

  return null;
}

// Partial council parser for streaming
export function parsePartialCouncil(text: string): Partial<CouncilResponses> | null {
  const result: Partial<CouncilResponses> = {};
  const providers = ['openai', 'deepseek', 'gemini', 'gpt-5.2'];

  for (const provider of providers) {
    // Match "provider": "content..." even if incomplete
    const pattern = new RegExp(`"${provider}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)`, 'i');
    const match = text.match(pattern);
    if (match) {
      // Unescape the string content
      const key = provider === 'gpt-5.2' ? 'openai' : provider;
      try {
        // Try to parse as complete JSON string
        result[key as keyof CouncilResponses] = JSON.parse(`"${match[1]}"`);
      } catch {
        // Use raw content if JSON parse fails (incomplete string)
        result[key as keyof CouncilResponses] = match[1].replace(/\\n/g, '\n').replace(/\\"/g, '"');
      }
    }
  }

  return Object.keys(result).length > 0 ? result : null;
}

// ============================================
// CODE BLOCK EXTRACTION
// ============================================

interface CodeBlock {
  language: string;
  code: string;
  start: number;
  end: number;
  isComplete: boolean;
}

export function extractCodeBlocks(text: string, isStreaming: boolean = false): {
  blocks: CodeBlock[];
  remaining: string;
} {
  const blocks: CodeBlock[] = [];

  // First, find all complete code blocks
  const pattern = /```(\w+)?\n([\s\S]*?)```/g;
  let match;

  while ((match = pattern.exec(text)) !== null) {
    blocks.push({
      language: match[1] || 'text',
      code: match[2].trim(),
      start: match.index,
      end: match.index + match[0].length,
      isComplete: true,
    });
  }

  // During streaming, also detect unclosed code blocks
  if (isStreaming) {
    // Check if there's an unclosed code block at the end
    const lastClosedEnd = blocks.length > 0 ? blocks[blocks.length - 1].end : 0;
    const remainingText = text.slice(lastClosedEnd);

    const unclosedMatch = remainingText.match(CODE_BLOCK_UNCLOSED);
    if (unclosedMatch) {
      blocks.push({
        language: unclosedMatch[1] || 'text',
        code: unclosedMatch[2],
        start: lastClosedEnd + (unclosedMatch.index || 0),
        end: text.length,
        isComplete: false,
      });
    }
  }

  // Remove code blocks from text to get remaining content
  let remaining = text;
  // Process in reverse order to maintain correct indices
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i];
    remaining = remaining.slice(0, block.start) + remaining.slice(block.end);
  }

  return { blocks, remaining: remaining.trim() };
}

// ============================================
// ERROR/WARNING DETECTION
// ============================================

interface ErrorMatch {
  message: string;
  code?: string;
  line: string;
}

export function detectErrors(text: string): ErrorMatch[] {
  const errors: ErrorMatch[] = [];
  const lines = text.split('\n');

  for (const line of lines) {
    const rustMatch = line.match(/^error(\[E\d+\])?:\s*(.+)$/i);
    if (rustMatch) {
      errors.push({
        message: rustMatch[2],
        code: rustMatch[1]?.slice(1, -1),
        line,
      });
      continue;
    }

    const genericMatch = line.match(/^(error|fatal|panic):\s*(.+)$/i);
    if (genericMatch) {
      errors.push({
        message: genericMatch[2],
        line,
      });
    }
  }

  return errors;
}

interface WarningMatch {
  message: string;
  code?: string;
  line: string;
}

export function detectWarnings(text: string): WarningMatch[] {
  const warnings: WarningMatch[] = [];
  const lines = text.split('\n');

  for (const line of lines) {
    const rustMatch = line.match(/^warning(\[\w+\])?:\s*(.+)$/i);
    if (rustMatch) {
      warnings.push({
        message: rustMatch[2],
        code: rustMatch[1]?.slice(1, -1),
        line,
      });
      continue;
    }

    const genericMatch = line.match(/^warn(ing)?:\s*(.+)$/i);
    if (genericMatch) {
      warnings.push({
        message: genericMatch[2],
        line,
      });
    }
  }

  return warnings;
}

// ============================================
// MAIN ENTRY POINT
// ============================================

/**
 * Parse text content into typed segments
 *
 * @param text - The raw text to parse
 * @param messageId - ID prefix for generated segment IDs
 * @param isStreaming - If true, use streaming mode (optimistic parsing, no caching)
 */
export function parseTextContent(
  text: string,
  messageId?: string,
  isStreaming: boolean = false
): ParseResult {
  // Empty check
  if (!text || text.trim() === '') {
    return {
      segments: [],
      metadata: { hasCode: false, hasErrors: false, hasWarnings: false, isCouncil: false },
    };
  }

  const idPrefix = messageId || 'parsed';

  // Only use cache for finalized content
  if (!isStreaming) {
    const cacheKey = `${PARSER_VERSION}:${fastHash(text)}`;
    const cached = parseCache.get(cacheKey);
    if (cached) {
      return {
        segments: cached,
        metadata: {
          hasCode: cached.some(s => s.type === 'code_block'),
          hasErrors: cached.some(s => s.type === 'error'),
          hasWarnings: cached.some(s => s.type === 'warning'),
          isCouncil: cached.some(s => s.type === 'council'),
        },
      };
    }
  }

  try {
    return isStreaming
      ? parseStreaming(text, idPrefix)
      : parseFinalized(text, idPrefix);
  } catch (error) {
    // FallbackToText on any parse failure
    console.warn('Parse failed, falling back to text:', error);
    return fallbackToText(text, idPrefix);
  }
}

/**
 * Streaming parser - fast, tolerant, handles incomplete content
 */
function parseStreaming(text: string, idPrefix: string): ParseResult {
  const segments: ParsedContent[] = [];

  // Check for incomplete council JSON first (show loading state)
  if (isIncompleteCouncilJson(text)) {
    // Try to extract any partial responses we can show
    const partial = parsePartialCouncil(text);
    segments.push({
      type: 'council_loading',
      id: generateId(idPrefix),
      partial: partial || undefined,
    });
    return {
      segments,
      metadata: { hasCode: false, hasErrors: false, hasWarnings: false, isCouncil: true },
    };
  }

  // Check for complete council response
  if (isCouncilResponse(text)) {
    const council = parseCouncilResponse(text);
    if (council) {
      segments.push({
        type: 'council',
        id: generateId(idPrefix),
        responses: council,
      });
      return {
        segments,
        metadata: { hasCode: false, hasErrors: false, hasWarnings: false, isCouncil: true },
      };
    }
  }

  // Extract code blocks (including unclosed ones)
  const { blocks: codeBlocks, remaining } = extractCodeBlocks(text, true);

  // Build ordered segments
  interface Segment {
    position: number;
    content: ParsedContent;
  }
  const orderedSegments: Segment[] = [];

  for (const block of codeBlocks) {
    orderedSegments.push({
      position: block.start,
      content: {
        type: 'code_block',
        id: generateId(idPrefix),
        language: block.language,
        code: block.code,
        // Mark incomplete blocks so UI can show streaming indicator
        ...(block.isComplete ? {} : { isStreaming: true }),
      } as ParsedContent,
    });
  }

  if (remaining.trim()) {
    orderedSegments.push({
      position: -1, // Text at start
      content: {
        type: 'text',
        id: generateId(idPrefix),
        content: remaining,
      },
    });
  }

  orderedSegments.sort((a, b) => a.position - b.position);
  for (const seg of orderedSegments) {
    segments.push(seg.content);
  }

  if (segments.length === 0) {
    segments.push({
      type: 'text',
      id: generateId(idPrefix),
      content: text,
    });
  }

  return {
    segments,
    metadata: {
      hasCode: codeBlocks.length > 0,
      hasErrors: false, // Skip error detection during streaming for speed
      hasWarnings: false,
      isCouncil: false,
    },
  };
}

/**
 * Finalized parser - strict, robust, caches result
 */
function parseFinalized(text: string, idPrefix: string): ParseResult {
  const segments: ParsedContent[] = [];

  // Check for council response
  if (isCouncilResponse(text)) {
    const council = parseCouncilResponse(text);
    if (council) {
      const result: ParsedContent[] = [{
        type: 'council',
        id: generateId(idPrefix),
        responses: council,
      }];
      parseCache.set(`${PARSER_VERSION}:${fastHash(text)}`, result);
      return {
        segments: result,
        metadata: { hasCode: false, hasErrors: false, hasWarnings: false, isCouncil: true },
      };
    }
  }

  // Extract code blocks (only complete ones)
  const { blocks: codeBlocks, remaining } = extractCodeBlocks(text, false);

  // Build ordered segments
  interface Segment {
    position: number;
    content: ParsedContent;
  }
  const orderedSegments: Segment[] = [];

  for (const block of codeBlocks) {
    orderedSegments.push({
      position: block.start,
      content: {
        type: 'code_block',
        id: generateId(idPrefix),
        language: block.language,
        code: block.code,
      },
    });
  }

  if (remaining.trim()) {
    // Check for errors/warnings in remaining text
    const errors = detectErrors(remaining);
    const warnings = detectWarnings(remaining);

    // For now, add remaining as text (could split on error/warning lines later)
    orderedSegments.push({
      position: -1,
      content: {
        type: 'text',
        id: generateId(idPrefix),
        content: remaining,
      },
    });
  }

  orderedSegments.sort((a, b) => a.position - b.position);
  for (const seg of orderedSegments) {
    segments.push(seg.content);
  }

  if (segments.length === 0) {
    segments.push({
      type: 'text',
      id: generateId(idPrefix),
      content: text,
    });
  }

  // Cache the result with versioned key
  parseCache.set(`${PARSER_VERSION}:${fastHash(text)}`, segments);

  return {
    segments,
    metadata: {
      hasCode: codeBlocks.length > 0,
      hasErrors: detectErrors(text).length > 0,
      hasWarnings: detectWarnings(text).length > 0,
      isCouncil: false,
    },
  };
}

/**
 * Fallback: render as plain text when parsing fails
 */
function fallbackToText(text: string, idPrefix: string): ParseResult {
  return {
    segments: [{
      type: 'text',
      id: generateId(idPrefix),
      content: text,
    }],
    metadata: { hasCode: false, hasErrors: false, hasWarnings: false, isCouncil: false },
  };
}

// ============================================
// TOOL OUTPUT PARSING
// ============================================

export function parseToolOutput(toolName: string, output: string): ParseResult {
  if (toolName === 'mcp__mira__hotline' || toolName === 'hotline') {
    if (isCouncilResponse(output)) {
      const council = parseCouncilResponse(output);
      if (council) {
        return {
          segments: [{
            type: 'council',
            id: generateId('tool'),
            responses: council,
          }],
          metadata: { hasCode: false, hasErrors: false, hasWarnings: false, isCouncil: true },
        };
      }
    }
  }

  return parseTextContent(output, 'tool', false);
}

// ============================================
// CACHE MANAGEMENT
// ============================================

export function clearParseCache(): void {
  parseCache.clear();
}

export function getCacheSize(): number {
  return parseCache.size;
}
