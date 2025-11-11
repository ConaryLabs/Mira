// src/components/MonacoEditor.tsx
// FIXED: Better font sizing and readability

import React from 'react';
import Editor from '@monaco-editor/react';

interface MonacoEditorProps {
  value: string;
  language: string;
  onChange?: (value: string | undefined) => void;
  options?: any;
  height?: string;
}

export const MonacoEditor: React.FC<MonacoEditorProps> = ({
  value,
  language,
  onChange,
  options = {},
  height = '100%'
}) => {
  const defaultOptions = {
    // Editor behavior
    minimap: { enabled: true, maxColumn: 80 }, // Enable minimap for navigation
    lineNumbers: 'on',
    scrollBeyondLastLine: false,
    automaticLayout: true,
    tabSize: 2,
    insertSpaces: true,
    wordWrap: 'on',
    wrappingIndent: 'indent',
    
    // READABILITY IMPROVEMENTS
    fontSize: 16, // Increased from 14 for better readability
    lineHeight: 24, // Better line spacing
    fontFamily: "'JetBrains Mono', 'Fira Code', 'Consolas', 'Courier New', monospace",
    fontLigatures: true, // Enable ligatures if font supports it
    
    // Padding and spacing
    padding: { top: 16, bottom: 16 },
    lineDecorationsWidth: 10,
    lineNumbersMinChars: 4,
    
    // Visual improvements
    renderLineHighlight: 'all',
    renderWhitespace: 'selection',
    cursorStyle: 'line',
    cursorBlinking: 'smooth',
    smoothScrolling: true,
    
    // Accessibility
    accessibilitySupport: 'auto',
    
    // Override with any user-specified options
    ...options
  };

  return (
    <Editor
      height={height}
      language={language}
      value={value}
      onChange={onChange}
      options={defaultOptions}
      theme="vs-dark"
      loading={
        <div className="flex items-center justify-center h-full">
          <div className="text-gray-400">Loading editor...</div>
        </div>
      }
    />
  );
};
