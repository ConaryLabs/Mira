/**
 * Expansion State Store
 *
 * Tracks which content blocks are expanded/collapsed.
 * Persists to localStorage so state survives page refresh.
 * Uses debounced writes to prevent localStorage thrashing.
 */

import { browser } from '$app/environment';

const STORAGE_KEY = 'mira-expansion-state';
const MAX_STORED_IDS = 500; // Limit storage size
const PERSIST_DEBOUNCE_MS = 1000; // Debounce localStorage writes

// In-memory state (updates immediately)
const expandedIds = new Set<string>();

// Debounce timer for localStorage writes
let persistTimeout: ReturnType<typeof setTimeout> | null = null;

// Load from localStorage on init
if (browser) {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const ids = JSON.parse(stored) as string[];
      ids.forEach(id => expandedIds.add(id));
    }
  } catch {
    // Ignore parse errors
  }
}

/**
 * Check if a segment is expanded
 */
export function isExpanded(segmentId: string): boolean {
  return expandedIds.has(segmentId);
}

/**
 * Toggle expansion state
 */
export function toggleExpanded(segmentId: string): boolean {
  if (expandedIds.has(segmentId)) {
    expandedIds.delete(segmentId);
  } else {
    expandedIds.add(segmentId);
  }
  persist();
  return expandedIds.has(segmentId);
}

/**
 * Set expansion state explicitly
 */
export function setExpanded(segmentId: string, expanded: boolean): void {
  if (expanded) {
    expandedIds.add(segmentId);
  } else {
    expandedIds.delete(segmentId);
  }
  persist();
}

/**
 * Expand all segments in a message
 */
export function expandAll(segmentIds: string[]): void {
  segmentIds.forEach(id => expandedIds.add(id));
  persist();
}

/**
 * Collapse all segments in a message
 */
export function collapseAll(segmentIds: string[]): void {
  segmentIds.forEach(id => expandedIds.delete(id));
  persist();
}

/**
 * Clear all expansion state
 */
export function clearExpansionState(): void {
  expandedIds.clear();
  if (browser) {
    localStorage.removeItem(STORAGE_KEY);
  }
}

/**
 * Persist to localStorage (debounced)
 */
function persist(): void {
  if (!browser) return;

  // Clear existing timer
  if (persistTimeout) {
    clearTimeout(persistTimeout);
  }

  // Debounce the write
  persistTimeout = setTimeout(() => {
    try {
      // Limit size by keeping only most recent IDs
      const ids = Array.from(expandedIds);
      const toStore = ids.slice(-MAX_STORED_IDS);
      localStorage.setItem(STORAGE_KEY, JSON.stringify(toStore));
    } catch {
      // Ignore storage errors (quota exceeded, etc.)
    }
    persistTimeout = null;
  }, PERSIST_DEBOUNCE_MS);
}

/**
 * Get reactive expansion state for Svelte 5
 * Returns a function that checks expansion and triggers reactivity
 */
export function createExpansionStore() {
  let version = $state(0);

  return {
    isExpanded(segmentId: string): boolean {
      // Reading version triggers reactivity
      void version;
      return expandedIds.has(segmentId);
    },

    toggle(segmentId: string): boolean {
      const newState = toggleExpanded(segmentId);
      version++; // Trigger reactivity
      return newState;
    },

    set(segmentId: string, expanded: boolean): void {
      setExpanded(segmentId, expanded);
      version++;
    },

    expandAll(segmentIds: string[]): void {
      expandAll(segmentIds);
      version++;
    },

    collapseAll(segmentIds: string[]): void {
      collapseAll(segmentIds);
      version++;
    },
  };
}
