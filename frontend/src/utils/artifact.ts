// src/utils/artifact.ts
// Consolidated artifact creation utilities

import type { Artifact } from '../stores/useChatStore';
import { detectLanguage } from './language';

/**
 * Normalize file path to consistent format
 */
export function normalizePath(rawPath?: string): string {
  if (!rawPath) return 'untitled';
  return String(rawPath)
    .replace(/\\/g, '/')        // Windows → POSIX
    .replace(/\/{2,}/g, '/')     // collapse duplicate slashes
    .replace(/^\.\/+/, '');     // strip leading ./
}

/**
 * Extract content from various payload formats
 */
export function extractContent(obj: any): string | undefined {
  return obj.content ?? obj.file_content ?? obj.text ?? obj.body ?? obj.value;
}

/**
 * Extract path from various payload formats
 */
export function extractPath(obj: any): string {
  const raw = obj.path ?? obj.file_path ?? obj.title ?? obj.name;
  return normalizePath(raw);
}

/**
 * Extract language from various payload formats
 */
export function extractLanguage(obj: any, fallbackPath: string): string {
  return obj.language ?? obj.programming_lang ?? detectLanguage(fallbackPath);
}

/**
 * Create an Artifact from a raw backend payload
 * Returns null if the payload doesn't contain valid artifact data
 */
export function createArtifact(
  obj: any,
  options?: {
    status?: Artifact['status'];
    origin?: Artifact['origin'];
    idPrefix?: string;
  }
): Artifact | null {
  if (!obj) return null;

  const content = extractContent(obj);
  if (!content || typeof content !== 'string') return null;

  const path = extractPath(obj);
  const language = extractLanguage(obj, path);

  return {
    id: obj.id || `${options?.idPrefix || 'artifact'}-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
    path,
    content,
    language,
    changeType: obj.change_type ?? obj.changeType,
    status: options?.status ?? obj.status ?? 'draft',
    origin: options?.origin ?? obj.origin ?? 'llm',
    timestamp: obj.timestamp ?? Date.now(),
    diff: obj.diff ?? undefined,
    isNewFile: obj.is_new_file ?? obj.isNewFile ?? undefined,
  };
}

/**
 * Extract artifacts from various backend message formats
 */
export function extractArtifacts(data: any, options?: {
  status?: Artifact['status'];
  origin?: Artifact['origin'];
  idPrefix?: string;
}): Artifact[] {
  if (!data) return [];

  const results: Artifact[] = [];

  // Direct artifact object
  if (data.artifact) {
    const a = createArtifact(data.artifact, options);
    if (a) results.push(a);
  }

  // Array of artifacts
  if (data.artifacts && Array.isArray(data.artifacts)) {
    data.artifacts.forEach((artifact: any) => {
      const a = createArtifact(artifact, options);
      if (a) results.push(a);
    });
  }

  // Other array fields that might contain artifacts
  const arrays = [data.files, data.results, data.output, data.outputs];
  for (const arr of arrays) {
    if (Array.isArray(arr) && results.length === 0) {
      arr.forEach((item: any) => {
        const a = createArtifact(item, options);
        if (a) results.push(a);
      });
      if (results.length > 0) break;
    }
  }

  // Single result object
  if (results.length === 0 && data.result) {
    const a = createArtifact(data.result, options);
    if (a) results.push(a);
  }

  // Fallback: try the data object itself
  if (results.length === 0) {
    const a = createArtifact(data, options);
    if (a) results.push(a);
  }

  return results;
}
