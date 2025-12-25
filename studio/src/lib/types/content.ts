// Content types for rich rendering

// Council response structure matching council tool output
export interface CouncilResponses {
  'gpt-5.2'?: string;
  'opus-4.5'?: string;
  'gemini-3-pro'?: string;
  [provider: string]: string | undefined;
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
