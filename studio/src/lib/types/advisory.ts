// Advisory session types matching backend API

export interface AdvisoryUsage {
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
}

export interface AdvisoryPricing {
  input_per_m: number;
  output_per_m: number;
  cache_read_per_m: number;
  reasoning_per_m: number;
}

export interface ProviderUsage {
  provider: string;
  model_id: string | null;
  display_name: string | null;
  usage: AdvisoryUsage;
  cost_usd: number;
  pricing: AdvisoryPricing | null;
}

export interface AdvisoryMessage {
  turn: number;
  role: string;
  provider: string | null;
  content: string;
  usage?: AdvisoryUsage;
  cost_usd?: number;
}

export interface AdvisoryPin {
  type: string;
  content: string;
}

export interface AdvisoryDecision {
  type: string;
  topic: string;
  rationale: string | null;
}

export interface AdvisorySessionSummary {
  id: string;
  topic: string | null;
  mode: string;
  status: string;
  total_turns: number;
  created_at: number; // Unix timestamp
}

export interface AdvisorySessionDetail {
  session: AdvisorySessionSummary;
  messages: AdvisoryMessage[];
  pins: AdvisoryPin[];
  decisions: AdvisoryDecision[];
  usage_by_provider: ProviderUsage[];
  total_cost_usd: number;
  deliberation_result?: Record<string, string>;
  model_metadata?: Record<string, ModelMetadata>;
  duration_seconds?: number;
}

export interface ModelMetadata {
  display_name: string;
  color: string;
  short_name: string;
}

// Provider colors for consistent UI
export const PROVIDER_COLORS: Record<string, string> = {
  'openai': '#10a37f',
  'anthropic': '#d4a574',
  'gemini': '#4285f4',
  'deepseek': '#5c6bc0',
};

// Short names for badges
export const PROVIDER_SHORT_NAMES: Record<string, string> = {
  'openai': 'GPT',
  'anthropic': 'Claude',
  'gemini': 'Gemini',
  'deepseek': 'DeepSeek',
};

// Format cost in USD
export function formatCost(cost: number): string {
  if (cost < 0.01) {
    return `$${(cost * 100).toFixed(3)}c`;
  }
  return `$${cost.toFixed(4)}`;
}

// Format token count
export function formatTokens(count: number): string {
  if (count >= 1000000) {
    return `${(count / 1000000).toFixed(1)}M`;
  }
  if (count >= 1000) {
    return `${(count / 1000).toFixed(1)}k`;
  }
  return count.toString();
}

// Format timestamp as relative time or date
export function formatTimestamp(unixTimestamp: number): string {
  const now = Date.now();
  const then = unixTimestamp * 1000; // Convert to milliseconds
  const diffMs = now - then;
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMins < 1) return 'just now';
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 7) return `${diffDays}d ago`;

  // Format as date for older items
  const date = new Date(then);
  return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}
