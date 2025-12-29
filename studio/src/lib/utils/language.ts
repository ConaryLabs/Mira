/**
 * Unified language detection utilities.
 * Provides both path-based (extension) and content-based detection.
 */

/**
 * Comprehensive extension to language mapping.
 * Consolidated from all frontend implementations.
 */
const extensionMap: Record<string, string> = {
  // TypeScript/JavaScript
  ts: 'typescript',
  tsx: 'tsx',
  mts: 'typescript',
  cts: 'typescript',
  js: 'javascript',
  jsx: 'jsx',
  mjs: 'javascript',
  cjs: 'javascript',

  // Rust
  rs: 'rust',

  // Python
  py: 'python',
  pyi: 'python',
  pyw: 'python',

  // Go
  go: 'go',

  // Shell/Bash
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  fish: 'bash',

  // Data formats
  json: 'json',
  jsonc: 'json',
  json5: 'json',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'toml',
  xml: 'xml',
  ini: 'ini',
  env: 'ini',

  // Markup
  html: 'html',
  htm: 'html',
  xhtml: 'html',
  svg: 'xml',
  svelte: 'svelte',
  vue: 'vue',

  // Styles
  css: 'css',
  scss: 'scss',
  sass: 'sass',
  less: 'less',

  // Markdown
  md: 'markdown',
  mdx: 'markdown',
  markdown: 'markdown',

  // SQL
  sql: 'sql',

  // Systems languages
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  cc: 'cpp',
  cxx: 'cpp',
  hpp: 'cpp',
  hxx: 'cpp',

  // JVM languages
  java: 'java',
  kt: 'kotlin',
  kts: 'kotlin',
  scala: 'scala',
  groovy: 'groovy',

  // Apple
  swift: 'swift',
  m: 'objectivec',
  mm: 'objectivec',

  // Other languages
  rb: 'ruby',
  lua: 'lua',
  php: 'php',
  pl: 'perl',
  r: 'r',
  dart: 'dart',
  zig: 'zig',
  nim: 'nim',
  ex: 'elixir',
  exs: 'elixir',
  erl: 'erlang',
  hs: 'haskell',
  clj: 'clojure',
  lisp: 'lisp',
  ml: 'ocaml',
  fs: 'fsharp',
  cs: 'csharp',
  vb: 'vb',

  // DevOps/Config
  dockerfile: 'docker',
  makefile: 'makefile',
  cmake: 'cmake',
  tf: 'hcl',
  hcl: 'hcl',
  nix: 'nix',

  // GraphQL
  graphql: 'graphql',
  gql: 'graphql',

  // Diff/Patch
  diff: 'diff',
  patch: 'diff',

  // Text
  txt: 'text',
  text: 'text',
  log: 'text',
};

/**
 * Special filename to language mapping for files without extensions.
 */
const filenameMap: Record<string, string> = {
  dockerfile: 'docker',
  makefile: 'makefile',
  cmakelists: 'cmake',
  gemfile: 'ruby',
  rakefile: 'ruby',
  vagrantfile: 'ruby',
  jenkinsfile: 'groovy',
  '.gitignore': 'gitignore',
  '.gitattributes': 'gitignore',
  '.dockerignore': 'gitignore',
  '.env': 'ini',
  '.env.local': 'ini',
  '.env.development': 'ini',
  '.env.production': 'ini',
  '.editorconfig': 'ini',
  '.prettierrc': 'json',
  '.eslintrc': 'json',
  'tsconfig.json': 'json',
  'package.json': 'json',
  'cargo.toml': 'toml',
  'pyproject.toml': 'toml',
};

/**
 * Detect language from a file path using extension.
 * Returns a normalized language identifier.
 */
export function detectLanguageFromPath(path: string): string {
  if (!path) return 'text';

  // Extract filename
  const filename = path.split('/').pop()?.toLowerCase() || '';

  // Check special filenames first
  const filenameNoExt = filename.replace(/\.[^.]+$/, '');
  if (filenameMap[filename]) return filenameMap[filename];
  if (filenameMap[filenameNoExt]) return filenameMap[filenameNoExt];

  // Extract extension
  const ext = filename.split('.').pop()?.toLowerCase() || '';

  return extensionMap[ext] || 'text';
}

/**
 * Detect language from code content using heuristics.
 * Best effort - may not always be accurate.
 */
export function detectLanguageFromContent(code: string): string {
  const trimmed = code.trim();
  if (!trimmed) return 'text';

  // JSON detection
  if (
    (trimmed.startsWith('{') && trimmed.endsWith('}')) ||
    (trimmed.startsWith('[') && trimmed.endsWith(']'))
  ) {
    try {
      JSON.parse(trimmed);
      return 'json';
    } catch {
      // Not valid JSON, continue
    }
  }

  // Rust detection
  if (
    /^(use\s+\w+|fn\s+\w+|impl\s+|struct\s+|enum\s+|mod\s+|pub\s+(fn|struct|enum|mod)|let\s+mut|#\[derive)/m.test(
      trimmed
    )
  ) {
    return 'rust';
  }

  // Python detection
  if (
    /^(def\s+\w+|class\s+\w+|import\s+\w+|from\s+\w+\s+import|if\s+__name__|@\w+)/m.test(trimmed) ||
    /^\s*#.*python/i.test(trimmed)
  ) {
    return 'python';
  }

  // TypeScript/JavaScript detection
  if (
    /^(import\s+.*from|export\s+(default\s+)?(function|class|const|let|var|interface|type)|const\s+\w+\s*=|let\s+\w+\s*=|function\s+\w+|interface\s+\w+|type\s+\w+)/m.test(
      trimmed
    )
  ) {
    // TypeScript indicators
    if (
      /:\s*(string|number|boolean|any|void|never|unknown)\b|interface\s+\w+|type\s+\w+\s*=|<[A-Z]\w*>/.test(
        trimmed
      )
    ) {
      return 'typescript';
    }
    return 'javascript';
  }

  // HTML/XML detection
  if (
    /^<(!DOCTYPE|html|head|body|div|span|p|a|script|style|link)/i.test(trimmed) ||
    /^<\?xml/.test(trimmed)
  ) {
    return 'html';
  }

  // SQL detection
  if (/^(SELECT|INSERT|UPDATE|DELETE|CREATE|DROP|ALTER|TRUNCATE|WITH|FROM)\s/i.test(trimmed)) {
    return 'sql';
  }

  // Shell detection
  if (
    /^#!\/bin\/(ba)?sh/m.test(trimmed) ||
    /^\s*\$\s+\w+/.test(trimmed) ||
    /^(sudo|cd|ls|grep|cat|echo|export|source|chmod|chown|mkdir|rm|cp|mv|git|npm|cargo|docker|kubectl)\s/m.test(
      trimmed
    )
  ) {
    return 'bash';
  }

  // YAML detection
  if (/^\w+:\s*(\n|$)/m.test(trimmed) && !trimmed.includes('{')) {
    return 'yaml';
  }

  // TOML detection
  if (/^\[[\w.-]+\]/m.test(trimmed)) {
    return 'toml';
  }

  // Diff detection
  if (/^(diff\s+--|@@\s+-\d+,\d+\s+\+\d+,\d+\s+@@|[+-]{3}\s+[ab]\/)/m.test(trimmed)) {
    return 'diff';
  }

  // Go detection
  if (/^(package\s+\w+|func\s+\w+|type\s+\w+\s+(struct|interface))/.test(trimmed)) {
    return 'go';
  }

  // CSS detection
  if (/^(\.|#|@media|@import|@keyframes|:root)\s*\w*\s*\{/m.test(trimmed)) {
    return 'css';
  }

  // Default to shell for command-like output
  if (/^[/~]\S+/.test(trimmed) || /error:|warning:|info:/i.test(trimmed)) {
    return 'bash';
  }

  return 'text';
}

/**
 * Detect language using both path and content.
 * Path takes precedence if available, falls back to content detection.
 */
export function detectLanguage(path?: string, content?: string): string {
  if (path) {
    const fromPath = detectLanguageFromPath(path);
    if (fromPath !== 'text') return fromPath;
  }

  if (content) {
    return detectLanguageFromContent(content);
  }

  return 'text';
}
