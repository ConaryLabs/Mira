// src/components/CodeBlock.tsx
import React, { useState } from 'react';
import { Copy, Check } from 'lucide-react';

interface CodeBlockProps {
  code: string;
  language?: string;
  isDark: boolean;
}

export const CodeBlock: React.FC<CodeBlockProps> = ({ code, language, isDark }) => {
  const [copied, setCopied] = useState(false);

  const copyToClipboard = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (error) {
      console.error('Failed to copy:', error);
    }
  };

  return (
    <div className={`
      relative group my-4 rounded-lg overflow-hidden border
      ${isDark 
        ? 'bg-gray-950 border-gray-700' 
        : 'bg-gray-50 border-gray-200'
      }
      shadow-sm
    `}>
      {/* Header bar with language and copy button */}
      <div className={`
        flex items-center justify-between px-4 py-2 border-b text-xs
        ${isDark 
          ? 'bg-gray-900 border-gray-700 text-gray-400' 
          : 'bg-gray-100 border-gray-200 text-gray-600'
        }
      `}>
        <span className="font-medium">
          {language || 'text'}
        </span>
        
        <button
          onClick={copyToClipboard}
          className={`
            flex items-center gap-1.5 px-2 py-1 rounded transition-colors
            ${isDark 
              ? 'hover:bg-gray-800 text-gray-400 hover:text-gray-300' 
              : 'hover:bg-gray-200 text-gray-600 hover:text-gray-700'
            }
          `}
          title="Copy code"
        >
          {copied ? (
            <>
              <Check size={12} className="text-green-500" />
              <span className="text-green-500">Copied</span>
            </>
          ) : (
            <>
              <Copy size={12} />
              <span>Copy</span>
            </>
          )}
        </button>
      </div>
      
      {/* Code content */}
      <div className="relative overflow-x-auto">
        <pre className={`
          p-4 text-sm leading-relaxed
          ${isDark ? 'text-gray-300' : 'text-gray-800'}
        `}>
          <code className="font-mono block">{code}</code>
        </pre>
      </div>
    </div>
  );
};
