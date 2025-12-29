/**
 * Unified text utilities for truncation and preview generation.
 */

/**
 * Truncate text by maximum number of lines.
 * Adds ellipsis indicator if truncated.
 */
export function truncateByLines(content: string, maxLines = 5): string {
  if (!content) return '';
  const lines = content.split('\n');
  if (lines.length <= maxLines) return content;
  return lines.slice(0, maxLines).join('\n') + '\n...';
}

/**
 * Truncate text by maximum character length.
 * Returns both the truncated text and whether truncation occurred.
 */
export function truncateByLength(
  content: string,
  maxLength = 2000
): { text: string; truncated: boolean } {
  if (!content || content.length <= maxLength) {
    return { text: content || '', truncated: false };
  }
  return { text: content.slice(0, maxLength), truncated: true };
}

/**
 * Create a preview of content for display.
 * Combines line and length truncation for optimal preview.
 */
export function createPreview(
  content: string,
  options: { maxLines?: number; maxLength?: number } = {}
): string {
  const { maxLines = 10, maxLength = 1000 } = options;
  if (!content) return '';

  // First truncate by lines
  let result = truncateByLines(content, maxLines);

  // Then truncate by length if still too long
  if (result.length > maxLength) {
    result = result.slice(0, maxLength) + '...';
  }

  return result;
}

/**
 * Format file size for display.
 */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

/**
 * Format duration for display.
 */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`;
}
