/**
 * Artifact Store - Tracks files and content created/modified during session
 *
 * Features:
 * - Track file reads, writes, and diffs
 * - Group by action type (read, write, diff)
 * - Link to source tool calls
 * - Preview content with lazy loading
 */

import type { ChatEvent, DiffInfo } from '$lib/api/client';
import { detectLanguageFromPath } from '$lib/utils/language';
import { truncateByLines } from '$lib/utils/text';

export type ArtifactKind = 'file' | 'diff' | 'patch' | 'log' | 'image';
export type ArtifactAction = 'read' | 'write' | 'modified' | 'created';

export interface Artifact {
  id: string;
  kind: ArtifactKind;
  action: ArtifactAction;
  title: string;
  path?: string;
  language?: string;
  preview: string;
  content?: string;
  totalBytes: number;
  sourceCallId?: string;
  sourceToolName?: string;
  createdAt: number;
  diff?: DiffInfo;
}

interface ArtifactStoreState {
  artifacts: Map<string, Artifact>;
  order: string[];  // Most recent first
}

function getFilename(path: string): string {
  return path.split('/').pop() || path;
}

function createArtifactStore() {
  let state = $state<ArtifactStoreState>({
    artifacts: new Map(),
    order: [],
  });

  // Derived: artifacts as array, most recent first
  const artifactList = $derived(
    state.order.map(id => state.artifacts.get(id)!).filter(Boolean)
  );

  // Derived: grouped by action
  const groupedArtifacts = $derived(() => {
    const groups: Record<ArtifactAction, Artifact[]> = {
      modified: [],
      created: [],
      write: [],
      read: [],
    };

    for (const artifact of artifactList) {
      groups[artifact.action].push(artifact);
    }

    return groups;
  });

  // Derived: count by action
  const counts = $derived({
    total: state.order.length,
    modified: artifactList.filter(a => a.action === 'modified').length,
    created: artifactList.filter(a => a.action === 'created').length,
    read: artifactList.filter(a => a.action === 'read').length,
  });

  function addArtifact(artifact: Omit<Artifact, 'id' | 'createdAt'>) {
    const id = `artifact-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const fullArtifact: Artifact = {
      ...artifact,
      id,
      createdAt: Date.now(),
    };

    // If same path exists, update instead of add
    const existingId = state.order.find(aid => {
      const a = state.artifacts.get(aid);
      return a?.path && a.path === artifact.path;
    });

    if (existingId) {
      // Update existing artifact
      state.artifacts.set(existingId, {
        ...fullArtifact,
        id: existingId,
      });
      // Move to front of order
      state.order = [existingId, ...state.order.filter(i => i !== existingId)];
    } else {
      // Add new artifact
      state.artifacts.set(id, fullArtifact);
      state.order = [id, ...state.order];
    }
  }

  function processToolResult(event: Extract<ChatEvent, { type: 'tool_call_result' }>) {
    const { name, call_id, output, diff } = event;

    // Handle file writes with diffs
    if (diff) {
      const path = diff.path;
      addArtifact({
        kind: 'diff',
        action: diff.is_new_file ? 'created' : 'modified',
        title: getFilename(path),
        path,
        language: detectLanguageFromPath(path),
        preview: truncateByLines(diff.new_content || '', 5),
        content: diff.new_content,
        totalBytes: (diff.new_content || '').length,
        sourceCallId: call_id,
        sourceToolName: name,
        diff,
      });
      return;
    }

    // Handle file reads
    if (name === 'read_file' || name === 'Read') {
      // Try to extract path from output or event
      const pathMatch = output.match(/^Reading: (.+?)$/m) || output.match(/^File: (.+?)$/m);
      const path = pathMatch?.[1] || 'unknown';

      if (output && !output.startsWith('Error')) {
        addArtifact({
          kind: 'file',
          action: 'read',
          title: getFilename(path),
          path,
          language: detectLanguageFromPath(path),
          preview: truncateByLines(output, 10),
          content: output,
          totalBytes: output.length,
          sourceCallId: call_id,
          sourceToolName: name,
        });
      }
    }

    // Handle glob results (file lists)
    if (name === 'glob' || name === 'Glob') {
      if (output && output.includes('\n')) {
        addArtifact({
          kind: 'log',
          action: 'read',
          title: 'File search results',
          preview: truncateByLines(output, 10),
          content: output,
          totalBytes: output.length,
          sourceCallId: call_id,
          sourceToolName: name,
        });
      }
    }

    // Handle shell output (significant output only)
    if ((name === 'bash' || name === 'Bash') && output.length > 100) {
      addArtifact({
        kind: 'log',
        action: 'read',
        title: 'Shell output',
        preview: truncateByLines(output, 10),
        content: output,
        totalBytes: output.length,
        sourceCallId: call_id,
        sourceToolName: name,
      });
    }
  }

  function getArtifact(id: string): Artifact | undefined {
    return state.artifacts.get(id);
  }

  function clear() {
    state.artifacts = new Map();
    state.order = [];
  }

  return {
    get artifacts() { return artifactList; },
    get grouped() { return groupedArtifacts(); },
    get counts() { return counts; },

    addArtifact,
    processToolResult,
    getArtifact,
    clear,
  };
}

export const artifactStore = createArtifactStore();

// Simple artifact viewer state (for modal)
interface ArtifactViewerState {
  isOpen: boolean;
  artifact: Artifact | null;
}

function createArtifactViewer() {
  let state = $state<ArtifactViewerState>({
    isOpen: false,
    artifact: null,
  });

  function open(artifact: Artifact) {
    state.artifact = artifact;
    state.isOpen = true;
  }

  // Legacy API for CodeBlock compatibility
  function openLegacy(data: { filename?: string; language: string; code: string }) {
    const artifact: Artifact = {
      id: 'viewer-temp',
      kind: 'file',
      action: 'read',
      title: data.filename || 'Code',
      language: data.language,
      preview: data.code.slice(0, 500),
      content: data.code,
      totalBytes: data.code.length,
      createdAt: Date.now(),
    };
    state.artifact = artifact;
    state.isOpen = true;
  }

  function close() {
    state.isOpen = false;
    state.artifact = null;
  }

  return {
    get isOpen() { return state.isOpen; },
    get artifact() { return state.artifact; },
    get filename() { return state.artifact?.title || ''; },
    get language() { return state.artifact?.language || 'text'; },
    get code() { return state.artifact?.content || state.artifact?.preview || ''; },

    open,
    openLegacy,
    close,
  };
}

export const artifactViewer = createArtifactViewer();

// Threshold for showing "Open in viewer" button
export const ARTIFACT_LINE_THRESHOLD = 50;
export const ARTIFACT_SIZE_THRESHOLD = 5000; // ~5KB

export function shouldShowViewerButton(code: string): boolean {
  const lineCount = code.split('\n').length;
  return lineCount > ARTIFACT_LINE_THRESHOLD || code.length > ARTIFACT_SIZE_THRESHOLD;
}
