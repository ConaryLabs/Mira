/**
 * Syntax highlighting using Prism.js
 * Supports multiple languages with auto-detection and theme integration.
 */

import Prism from 'prismjs';
import { detectLanguageFromContent } from './language';

// Import language support - Prism loads JavaScript by default
import 'prismjs/components/prism-typescript';
import 'prismjs/components/prism-rust';
import 'prismjs/components/prism-python';
import 'prismjs/components/prism-bash';
import 'prismjs/components/prism-shell-session';
import 'prismjs/components/prism-json';
import 'prismjs/components/prism-yaml';
import 'prismjs/components/prism-toml';
import 'prismjs/components/prism-markdown';
import 'prismjs/components/prism-sql';
import 'prismjs/components/prism-css';
import 'prismjs/components/prism-scss';
import 'prismjs/components/prism-markup'; // HTML
import 'prismjs/components/prism-jsx';
import 'prismjs/components/prism-tsx';
import 'prismjs/components/prism-diff';
import 'prismjs/components/prism-go';
import 'prismjs/components/prism-c';
import 'prismjs/components/prism-cpp';
import 'prismjs/components/prism-java';
import 'prismjs/components/prism-kotlin';
import 'prismjs/components/prism-swift';
import 'prismjs/components/prism-ruby';
import 'prismjs/components/prism-lua';
import 'prismjs/components/prism-docker';
import 'prismjs/components/prism-graphql';
import 'prismjs/components/prism-regex';
import 'prismjs/components/prism-ini';

// Language aliases mapping user-provided language names to Prism grammar names
const languageAliases: Record<string, string> = {
  // TypeScript/JavaScript
  ts: 'typescript',
  tsx: 'tsx',
  js: 'javascript',
  jsx: 'jsx',
  javascript: 'javascript',
  typescript: 'typescript',

  // Rust
  rs: 'rust',
  rust: 'rust',

  // Python
  py: 'python',
  python: 'python',

  // Shell/Bash
  sh: 'bash',
  bash: 'bash',
  shell: 'bash',
  zsh: 'bash',
  fish: 'bash',
  console: 'shell-session',
  terminal: 'shell-session',

  // Data formats
  json: 'json',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'toml',
  xml: 'markup',
  ini: 'ini',
  conf: 'ini',
  config: 'ini',

  // Markup
  html: 'markup',
  htm: 'markup',
  svg: 'markup',
  svelte: 'markup', // Svelte uses HTML-like syntax
  vue: 'markup',

  // Styles
  css: 'css',
  scss: 'scss',
  sass: 'scss',
  less: 'css',

  // Markdown
  md: 'markdown',
  markdown: 'markdown',

  // SQL
  sql: 'sql',
  mysql: 'sql',
  postgres: 'sql',
  postgresql: 'sql',
  sqlite: 'sql',

  // Systems languages
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  'c++': 'cpp',
  cc: 'cpp',
  cxx: 'cpp',
  hpp: 'cpp',

  // JVM languages
  java: 'java',
  kt: 'kotlin',
  kotlin: 'kotlin',
  scala: 'java', // Close enough

  // Apple
  swift: 'swift',
  objc: 'c',
  'm': 'c',

  // Other languages
  go: 'go',
  golang: 'go',
  rb: 'ruby',
  ruby: 'ruby',
  lua: 'lua',

  // DevOps
  dockerfile: 'docker',
  docker: 'docker',

  // GraphQL
  graphql: 'graphql',
  gql: 'graphql',

  // Diff
  diff: 'diff',
  patch: 'diff',

  // Regex
  regex: 'regex',
  regexp: 'regex',

  // Plain text
  text: 'plain',
  txt: 'plain',
  plain: 'plain',
  '': 'plain',
};

/**
 * Get the Prism grammar name for a language string.
 */
function getGrammarName(language: string): string {
  const normalized = language.toLowerCase().trim();
  return languageAliases[normalized] || normalized;
}

/**
 * Detect language from code content using heuristics.
 * Re-exported from language.ts for backward compatibility.
 */
export function detectLanguage(code: string): string {
  // Map unified language names to Prism grammar names
  const lang = detectLanguageFromContent(code);
  return lang === 'text' ? 'plain' : lang === 'html' ? 'markup' : lang;
}

/**
 * Highlight code using Prism.js.
 * Returns HTML string with syntax highlighting spans.
 */
export function highlightCode(code: string, language?: string): string {
  const lang = language ? getGrammarName(language) : detectLanguage(code);

  // Handle plain text - just escape HTML
  if (lang === 'plain' || !Prism.languages[lang]) {
    return escapeHtml(code);
  }

  try {
    return Prism.highlight(code, Prism.languages[lang], lang);
  } catch (e) {
    console.warn(`Prism highlighting failed for language "${lang}":`, e);
    return escapeHtml(code);
  }
}

/**
 * Escape HTML to prevent XSS.
 */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/**
 * Legacy highlight function for backward compatibility with ToolInvocation.
 * Auto-detects JSON vs shell output and applies highlighting.
 */
export function highlight(code: string): string {
  return highlightCode(code);
}

/**
 * Human-readable language display names.
 */
export const languageDisplayNames: Record<string, string> = {
  typescript: 'TypeScript',
  javascript: 'JavaScript',
  tsx: 'TSX',
  jsx: 'JSX',
  rust: 'Rust',
  python: 'Python',
  bash: 'Bash',
  'shell-session': 'Shell',
  json: 'JSON',
  yaml: 'YAML',
  toml: 'TOML',
  markup: 'HTML',
  css: 'CSS',
  scss: 'SCSS',
  sql: 'SQL',
  markdown: 'Markdown',
  go: 'Go',
  c: 'C',
  cpp: 'C++',
  java: 'Java',
  kotlin: 'Kotlin',
  swift: 'Swift',
  ruby: 'Ruby',
  lua: 'Lua',
  docker: 'Dockerfile',
  graphql: 'GraphQL',
  diff: 'Diff',
  ini: 'INI',
  plain: 'Plain Text',
};

/**
 * Get display name for a language.
 */
export function getLanguageDisplayName(language: string): string {
  const grammarName = getGrammarName(language);
  return languageDisplayNames[grammarName] || language.toUpperCase();
}
