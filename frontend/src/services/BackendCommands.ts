// src/services/BackendCommands.ts
// FIXED: Uses centralized config for session ID

import { useWebSocketStore } from '../stores/useWebSocketStore';
import { getSessionId } from '../config/app';

export class BackendCommands {
  private send: (message: any) => Promise<void>;

  constructor() {
    // Get send from the Zustand store
    this.send = useWebSocketStore.getState().send;
  }

  // ==================== PROJECT COMMANDS ====================
  
  async listProjects() {
    return this.send({
      type: 'project_command',
      method: 'project.list',
      params: {}
    });
  }

  async createProject(name: string, description?: string, tags?: string[]) {
    return this.send({
      type: 'project_command',
      method: 'project.create',
      params: { name, description, tags }
    });
  }

  async setActiveProject(projectId: string) {
    return this.send({
      type: 'project_command',
      method: 'project.set_active',
      params: { project_id: projectId }
    });
  }

  async updateProject(projectId: string, updates: any) {
    return this.send({
      type: 'project_command',
      method: 'project.update',
      params: { project_id: projectId, ...updates }
    });
  }

  async deleteProject(projectId: string) {
    return this.send({
      type: 'project_command',
      method: 'project.delete',
      params: { project_id: projectId }
    });
  }

  // ==================== GIT COMMANDS ====================

  async importRepository(projectId: string, gitUrl?: string, localPath?: string) {
    return this.send({
      type: 'git_command',
      method: 'git.import',
      params: { project_id: projectId, git_url: gitUrl, local_path: localPath }
    });
  }

  async gitSync(projectId: string, message?: string) {
    return this.send({
      type: 'git_command',
      method: 'git.sync',
      params: { project_id: projectId, message: message || 'Update from Mira' }
    });
  }

  async gitStatus(projectId: string) {
    return this.send({
      type: 'git_command',
      method: 'git.status',
      params: { project_id: projectId }
    });
  }

  async gitCommit(projectId: string, message: string, files?: string[]) {
    return this.send({
      type: 'git_command',
      method: 'git.commit',
      params: { project_id: projectId, message, files }
    });
  }

  async gitPush(projectId: string, branch?: string) {
    return this.send({
      type: 'git_command',
      method: 'git.push',
      params: { project_id: projectId, branch }
    });
  }

  async gitPull(projectId: string, branch?: string) {
    return this.send({
      type: 'git_command',
      method: 'git.pull',
      params: { project_id: projectId, branch }
    });
  }

  async gitBranch(projectId: string, branchName?: string) {
    return this.send({
      type: 'git_command',
      method: 'git.branch',
      params: { project_id: projectId, branch_name: branchName }
    });
  }

  async gitCheckout(projectId: string, branch: string) {
    return this.send({
      type: 'git_command',
      method: 'git.checkout',
      params: { project_id: projectId, branch }
    });
  }

  async gitReset(projectId: string, mode: 'soft' | 'mixed' | 'hard' = 'mixed') {
    return this.send({
      type: 'git_command',
      method: 'git.reset',
      params: { project_id: projectId, mode }
    });
  }

  async gitRestore(projectId: string, filePath: string) {
    return this.send({
      type: 'git_command',
      method: 'git.restore',
      params: { project_id: projectId, file_path: filePath }
    });
  }

  async getFileTree(projectId: string) {
    return this.send({
      type: 'git_command',
      method: 'git.tree',
      params: { project_id: projectId }
    });
  }

  async getFileContent(projectId: string, filePath: string) {
    return this.send({
      type: 'git_command',
      method: 'git.file',
      params: { project_id: projectId, file_path: filePath }
    });
  }

  // ==================== FILE SYSTEM COMMANDS ====================

  async writeFile(projectId: string, path: string, content: string) {
    return this.send({
      type: 'file_system_command',
      method: 'files.write',
      params: { project_id: projectId, path, content }
    });
  }

  async readFile(projectId: string, path: string) {
    return this.send({
      type: 'file_system_command',
      method: 'files.read',
      params: { project_id: projectId, path }
    });
  }

  async listFiles(projectId: string, path: string) {
    return this.send({
      type: 'file_system_command',
      method: 'files.list',
      params: { project_id: projectId, path }
    });
  }

  async deleteFile(projectId: string, path: string) {
    return this.send({
      type: 'file_system_command',
      method: 'files.delete',
      params: { project_id: projectId, path }
    });
  }

  async createDirectory(projectId: string, path: string) {
    return this.send({
      type: 'file_system_command',
      method: 'directory.create',
      params: { project_id: projectId, path }
    });
  }

  async searchFiles(query: string, projectId?: string) {
    return this.send({
      type: 'file_system_command',
      method: 'files.search',
      params: { query, project_id: projectId }
    });
  }

  // ==================== CODE INTELLIGENCE COMMANDS ====================

  async searchCode(pattern: string, projectId: string, limit: number = 10) {
    return this.send({
      type: 'code_intelligence',
      method: 'code.search',
      params: { 
        pattern, 
        project_id: projectId,
        limit 
      }
    });
  }

  async getComplexityHotspots(projectId: string, limit: number = 5) {
    return this.send({
      type: 'code_intelligence',
      method: 'code.complexity_hotspots',
      params: { 
        project_id: projectId,
        limit 
      }
    });
  }

  async getRepositoryStats(projectId: string) {
    return this.send({
      type: 'code_intelligence',
      method: 'code.stats',
      params: { project_id: projectId }
    });
  }

  async analyzeFile(projectId: string, filePath: string) {
    return this.send({
      type: 'code_intelligence',
      method: 'code.analyze',
      params: { 
        project_id: projectId,
        file_path: filePath 
      }
    });
  }

  // ==================== DOCUMENT PROCESSING COMMANDS ====================

  async uploadDocument(projectId: string, fileName: string, content: string) {
    return this.send({
      type: 'document_command',
      method: 'documents.upload',
      params: { project_id: projectId, file_name: fileName, content }
    });
  }

  async searchDocuments(projectId: string, query: string, limit: number = 10) {
    return this.send({
      type: 'document_command',
      method: 'documents.search',
      params: { project_id: projectId, query, limit }
    });
  }

  async listDocuments(projectId: string) {
    return this.send({
      type: 'document_command',
      method: 'documents.list',
      params: { project_id: projectId }
    });
  }

  async retrieveDocument(documentId: string) {
    return this.send({
      type: 'document_command',
      method: 'documents.retrieve',
      params: { document_id: documentId }
    });
  }

  async deleteDocument(documentId: string) {
    return this.send({
      type: 'document_command',
      method: 'documents.delete',
      params: { document_id: documentId }
    });
  }

  // ==================== CHAT / MESSAGE COMMANDS ====================

  async sendChat(message: string, projectId?: string, metadata?: any) {
    // FIXED: Use centralized config instead of hardcoded value
    const fullMetadata: any = {
      session_id: getSessionId(),
      ...metadata
    };

    return this.send({
      type: 'chat',
      content: message,
      project_id: projectId,
      metadata: fullMetadata
    });
  }

  // ==================== MEMORY COMMANDS ====================

  async searchMemory(sessionId: string, query: string, limit: number = 10) {
    return this.send({
      type: 'memory',
      method: 'memory.search',
      params: {
        session_id: sessionId,
        query,
        limit
      }
    });
  }

  async getMemoryStats(sessionId: string) {
    return this.send({
      type: 'memory',
      method: 'memory.stats',
      params: {
        session_id: sessionId
      }
    });
  }

  // ==================== TERMINAL COMMANDS ====================

  async startTerminal(projectId: string, workingDirectory?: string, cols: number = 80, rows: number = 24) {
    return this.send({
      type: 'terminal_command',
      method: 'start_session',
      params: {
        project_id: projectId,
        working_directory: workingDirectory,
        cols,
        rows
      }
    });
  }

  async sendTerminalInput(sessionId: string, data: string) {
    return this.send({
      type: 'terminal_command',
      method: 'send_input',
      params: {
        session_id: sessionId,
        data  // Base64 encoded
      }
    });
  }

  async resizeTerminal(sessionId: string, cols: number, rows: number) {
    return this.send({
      type: 'terminal_command',
      method: 'resize',
      params: {
        session_id: sessionId,
        cols,
        rows
      }
    });
  }

  async closeTerminal(sessionId: string) {
    return this.send({
      type: 'terminal_command',
      method: 'close_session',
      params: {
        session_id: sessionId
      }
    });
  }

  async listTerminalSessions(projectId: string, activeOnly: boolean = true, limit?: number) {
    return this.send({
      type: 'terminal_command',
      method: 'list_sessions',
      params: {
        project_id: projectId,
        active_only: activeOnly,
        limit
      }
    });
  }

  // ==================== RUN PROJECT COMMAND ====================

  async runProject(projectId: string, command?: string) {
    // Determine command based on project type
    if (!command) {
      command = 'cargo run';  // Default to Rust
    }

    return this.send({
      type: 'project_command',
      method: 'project.run',
      params: {
        project_id: projectId,
        command
      }
    });
  }

  // ==================== UTILITY COMMANDS ====================

  async processCommand(input: string, projectId?: string) {
    const lower = input.toLowerCase().trim();

    // Git operations
    if (lower.match(/^(commit|git commit)/)) {
      const message = input.replace(/^(commit|git commit)\s*/i, '') || 'Quick update';
      return this.gitCommit(projectId!, message);
    }

    if (lower.match(/^(push|git push)/)) {
      return this.gitPush(projectId!);
    }

    if (lower.match(/^(pull|git pull)/)) {
      return this.gitPull(projectId!);
    }

    if (lower.match(/^(git )?status/)) {
      return this.gitStatus(projectId!);
    }

    // File operations
    if (lower.startsWith('create ') && lower.includes('file')) {
      const filename = input.replace(/^create\s+file\s+/i, '');
      return this.writeFile(projectId!, filename, '// New file\n');
    }

    if (lower.startsWith('search ')) {
      const query = input.replace(/^search\s+/i, '');
      return this.searchCode(query, projectId!, 20);
    }

    // Project operations
    if (lower.startsWith('import ') && (lower.includes('repo') || lower.includes('git'))) {
      const url = input.replace(/^import\s+(repo|repository|git)\s+/i, '');
      return this.importRepository(projectId!, url);
    }

    // Default to chat
    return this.sendChat(input, projectId);
  }
}

// Hook to use backend commands
export const useBackendCommands = () => {
  return new BackendCommands();
};
