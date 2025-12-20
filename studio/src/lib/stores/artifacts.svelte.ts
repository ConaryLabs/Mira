/**
 * Artifact viewer state management
 */

interface ArtifactState {
  isOpen: boolean;
  filename?: string;
  language: string;
  code: string;
}

const defaultState: ArtifactState = {
  isOpen: false,
  language: '',
  code: '',
};

// Svelte 5 runes-based store
let state = $state<ArtifactState>({ ...defaultState });

export const artifactViewer = {
  get isOpen() { return state.isOpen; },
  get filename() { return state.filename; },
  get language() { return state.language; },
  get code() { return state.code; },

  open(artifact: { filename?: string; language: string; code: string }) {
    state = {
      isOpen: true,
      ...artifact,
    };
  },

  close() {
    state = { ...defaultState };
  },
};

// Threshold for showing "Open in viewer" button
export const ARTIFACT_LINE_THRESHOLD = 50;
export const ARTIFACT_SIZE_THRESHOLD = 5000; // ~5KB

export function shouldShowViewerButton(code: string): boolean {
  const lineCount = code.split('\n').length;
  return lineCount > ARTIFACT_LINE_THRESHOLD || code.length > ARTIFACT_SIZE_THRESHOLD;
}
