// Parsed content types for rich rendering
// Each variant represents a distinct content type that gets specialized UI treatment

export type ParsedContent =
  | { type: 'text'; id: string; content: string }
  | { type: 'code_block'; id: string; language: string; code: string; filename?: string }
  | { type: 'council'; id: string; responses: CouncilResponses }
  | { type: 'council_loading'; id: string; partial?: Partial<CouncilResponses> }
  | { type: 'error'; id: string; message: string; code?: string }
  | { type: 'warning'; id: string; message: string }
  | { type: 'diff'; id: string; path: string; oldContent?: string; newContent: string; isNewFile: boolean };

// Council response structure matching council tool output
export interface CouncilResponses {
  'gpt-5.2'?: string;
  'opus-4.5'?: string;
  'gemini-3-pro'?: string;
  [provider: string]: string | undefined;
}

// Content classification for two-pass parsing
export type ContentKind =
  | 'code_block'
  | 'council'
  | 'council_loading'
  | 'error'
  | 'warning'
  | 'diff'
  | 'text';

// Parse result with metadata
export interface ParseResult {
  segments: ParsedContent[];
  metadata: {
    hasCode: boolean;
    hasErrors: boolean;
    hasWarnings: boolean;
    isCouncil: boolean;
  };
}

// Provider display info for council cards
export interface ProviderInfo {
  key: string;
  displayName: string;
  response: string;
}

export const PROVIDER_DISPLAY_NAMES: Record<string, string> = {
  'gpt-5.2': 'GPT 5.2',
  'opus-4.5': 'Claude Opus 4.5',
  'gemini-3-pro': 'Gemini 3 Pro',
};
