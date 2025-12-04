// src/types/index.ts
// SCORCHED EARTH: Only types actually used, no legacy bullshit

// ===== SESSION =====
export interface Session {
  id: string;
  user_id?: string;
  name?: string;
  project_path?: string;
  last_message_preview?: string;
  message_count: number;
  created_at: number;  // Unix timestamp
  last_active: number; // Unix timestamp
}

// ===== PROJECT =====
export interface Project {
  id: string;
  name: string;
  description?: string;
  tags?: string[];
  owner?: string;
  
  // Backend sends these as snake_case (matching Rust serialization)
  has_repository?: boolean;
  has_codebase?: boolean;
  repository_url?: string;
  import_status?: string;
  last_sync_at?: string | null;
  
  // Backend sends DateTime<Utc> as RFC3339 string
  created_at: string;
  updated_at: string;
}

// ===== DOCUMENTS =====
export interface DocumentMetadata {
  id: string;
  file_name: string;
  file_type: string;
  size_bytes: number;
  chunk_count: number;
  word_count?: number;
  created_at: string;
  project_id?: string;
}

export interface DocumentSearchResult {
  chunk_id: string;
  chunk_index: number;
  document_id: string;
  file_name: string;
  content: string;
  score: number;
  page_number?: number;
}

// ===== FILE SYSTEM =====
export interface FileSystemResponse {
  files?: FileNode[];
  content?: string;
  success?: boolean;
  error?: string;
}

export interface FileNode {
  name: string;
  path: string;
  type: 'file' | 'directory';
  children?: FileNode[];
  size?: number;
  modified?: string;
}

// ===== TOOL RESULTS =====
export interface ToolResult {
  id: string;
  type: 'web_search' | 'code_execution' | 'file_operation' | 'git_operation' | 'code_analysis' | 'code_search' | 'repository_stats' | 'complexity_analysis';
  status: 'success' | 'error' | 'pending';
  data: any;
  timestamp: number;
}

// ===== WEBSOCKET =====
export interface WebSocketMessage {
  type: string;
  [key: string]: any;
}
