// src/components/UnifiedDiffView.tsx
// Git-style unified diff viewer component

import React, { useMemo } from 'react';

interface UnifiedDiffViewProps {
  diff: string;
  compact?: boolean; // For inline chat display (smaller, limited height)
}

interface DiffLine {
  type: 'header' | 'hunk' | 'addition' | 'deletion' | 'context';
  content: string;
  lineNumber?: number;
}

/**
 * Parse diff stats from unified diff string
 */
export function parseDiffStats(diff: string): { additions: number; deletions: number } {
  const lines = diff.split('\n');
  let additions = 0;
  let deletions = 0;

  for (const line of lines) {
    if (line.startsWith('+') && !line.startsWith('+++')) {
      additions++;
    } else if (line.startsWith('-') && !line.startsWith('---')) {
      deletions++;
    }
  }

  return { additions, deletions };
}

/**
 * Parse unified diff into structured lines
 */
function parseDiffLines(diff: string): DiffLine[] {
  const lines = diff.split('\n');
  const result: DiffLine[] = [];

  for (const line of lines) {
    if (line.startsWith('---') || line.startsWith('+++')) {
      result.push({ type: 'header', content: line });
    } else if (line.startsWith('@@')) {
      result.push({ type: 'hunk', content: line });
    } else if (line.startsWith('+')) {
      result.push({ type: 'addition', content: line });
    } else if (line.startsWith('-')) {
      result.push({ type: 'deletion', content: line });
    } else {
      result.push({ type: 'context', content: line });
    }
  }

  return result;
}

/**
 * Diff stats badge component
 */
export const DiffStats: React.FC<{ diff: string; className?: string }> = ({ diff, className = '' }) => {
  const { additions, deletions } = useMemo(() => parseDiffStats(diff), [diff]);

  return (
    <span className={`text-xs font-mono ${className}`}>
      <span className="text-green-400">+{additions}</span>
      {' '}
      <span className="text-red-400">-{deletions}</span>
    </span>
  );
};

export const UnifiedDiffView: React.FC<UnifiedDiffViewProps> = ({ diff, compact = false }) => {
  const lines = useMemo(() => parseDiffLines(diff), [diff]);

  const getLineClass = (type: DiffLine['type']): string => {
    switch (type) {
      case 'addition':
        return 'bg-green-900/40 text-green-300';
      case 'deletion':
        return 'bg-red-900/40 text-red-300';
      case 'hunk':
        return 'bg-blue-900/40 text-blue-300';
      case 'header':
        return 'text-gray-500 font-bold';
      default:
        return 'text-gray-300';
    }
  };

  if (compact) {
    return (
      <div className="font-mono text-xs overflow-auto max-h-48 rounded bg-gray-900/50">
        {lines.map((line, i) => (
          <div
            key={i}
            className={`px-2 py-0.5 ${getLineClass(line.type)} whitespace-pre overflow-x-auto`}
          >
            {line.content || ' '}
          </div>
        ))}
      </div>
    );
  }

  return (
    <div className="font-mono text-sm overflow-auto h-full bg-gray-950">
      {lines.map((line, i) => (
        <div
          key={i}
          className={`px-4 py-0.5 ${getLineClass(line.type)} whitespace-pre`}
        >
          {line.content || ' '}
        </div>
      ))}
    </div>
  );
};

export default UnifiedDiffView;
