// Shared utility for detecting programming language from file paths
// Consolidated from multiple hook implementations

/**
 * Detects the programming language based on file extension
 * @param path - The file path to analyze
 * @returns The language identifier (e.g., 'rust', 'typescript', 'plaintext')
 */
export function detectLanguage(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase();

  switch (ext) {
    case 'rs': return 'rust';
    case 'js': case 'jsx': return 'javascript';
    case 'ts': case 'tsx': return 'typescript';
    case 'py': return 'python';
    case 'java': return 'java';
    case 'c': return 'c';
    case 'cpp': case 'cc': case 'cxx': return 'cpp';
    case 'cs': return 'csharp';
    case 'go': return 'go';
    case 'rb': return 'ruby';
    case 'php': return 'php';
    case 'swift': return 'swift';
    case 'kt': case 'kts': return 'kotlin';
    case 'json': return 'json';
    case 'xml': return 'xml';
    case 'html': case 'htm': return 'html';
    case 'css': return 'css';
    case 'scss': case 'sass': return 'scss';
    case 'md': case 'markdown': return 'markdown';
    case 'toml': return 'toml';
    case 'yaml': case 'yml': return 'yaml';
    case 'sh': case 'bash': case 'zsh': return 'shell';
    case 'sql': return 'sql';
    case 'graphql': case 'gql': return 'graphql';
    case 'dockerfile': return 'dockerfile';
    case 'proto': return 'protobuf';
    default: return 'plaintext';
  }
}
