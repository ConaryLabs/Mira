// src/stores/useSudoStore.ts
// State management for sudo approval requests and permissions

import { create } from 'zustand';
import { useWebSocketStore } from './useWebSocketStore';

// ============================================================================
// TYPES
// ============================================================================

export interface SudoApprovalRequest {
  id: string;
  operationId?: string;
  sessionId: string;
  command: string;
  reason?: string;
  expiresAt: number;
  status: 'pending' | 'approved' | 'denied' | 'expired';
  timestamp: number;
}

export interface SudoPermission {
  id: number;
  name: string;
  description?: string;
  command_exact?: string;
  command_pattern?: string;
  command_prefix?: string;
  requires_approval: boolean;
  enabled: boolean;
  use_count: number;
  last_used_at?: number;
}

export interface SudoBlocklistEntry {
  id: number;
  name: string;
  description?: string;
  pattern_exact?: string;
  pattern_regex?: string;
  pattern_prefix?: string;
  severity: string;
  is_default: boolean;
  enabled: boolean;
}

// ============================================================================
// STORE
// ============================================================================

interface SudoStore {
  // State
  pendingApprovals: SudoApprovalRequest[];
  permissions: SudoPermission[];
  blocklist: SudoBlocklistEntry[];
  loading: boolean;

  // Actions - Approval management
  addPendingApproval: (request: SudoApprovalRequest) => void;
  removePendingApproval: (id: string) => void;
  updateApprovalStatus: (id: string, status: SudoApprovalRequest['status']) => void;
  clearExpired: () => void;

  // Actions - WebSocket commands
  approveRequest: (id: string) => Promise<void>;
  denyRequest: (id: string, reason?: string) => Promise<void>;
  listPending: (sessionId: string) => Promise<void>;

  // Actions - Permission management
  fetchPermissions: () => Promise<void>;
  addPermission: (permission: Partial<SudoPermission>) => Promise<void>;
  removePermission: (id: number) => Promise<void>;
  togglePermission: (id: number, enabled: boolean) => Promise<void>;

  // Actions - Blocklist management
  fetchBlocklist: () => Promise<void>;
  addBlocklistEntry: (entry: Partial<SudoBlocklistEntry>) => Promise<void>;
  removeBlocklistEntry: (id: number) => Promise<void>;
  toggleBlocklistEntry: (id: number, enabled: boolean) => Promise<void>;

  // Actions - Update from server
  setPermissions: (permissions: SudoPermission[]) => void;
  setBlocklist: (blocklist: SudoBlocklistEntry[]) => void;
  setLoading: (loading: boolean) => void;
}

export const useSudoStore = create<SudoStore>((set, get) => ({
  // Initial state
  pendingApprovals: [],
  permissions: [],
  blocklist: [],
  loading: false,

  // Approval management
  addPendingApproval: (request) => {
    set((state) => ({
      pendingApprovals: [
        // Remove any existing request with same ID (in case of re-send)
        ...state.pendingApprovals.filter((r) => r.id !== request.id),
        request,
      ],
    }));
  },

  removePendingApproval: (id) => {
    set((state) => ({
      pendingApprovals: state.pendingApprovals.filter((r) => r.id !== id),
    }));
  },

  updateApprovalStatus: (id, status) => {
    set((state) => ({
      pendingApprovals: state.pendingApprovals.map((r) =>
        r.id === id ? { ...r, status } : r
      ),
    }));
  },

  clearExpired: () => {
    const now = Date.now() / 1000;
    set((state) => ({
      pendingApprovals: state.pendingApprovals.filter(
        (r) => r.status === 'pending' && r.expiresAt > now
      ),
    }));
  },

  // WebSocket commands
  approveRequest: async (id) => {
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.approve',
      params: { approval_request_id: id },
    });
  },

  denyRequest: async (id, reason) => {
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.deny',
      params: { approval_request_id: id, reason },
    });
  },

  listPending: async (sessionId) => {
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.list_pending',
      params: { session_id: sessionId },
    });
  },

  // Permission management
  fetchPermissions: async () => {
    set({ loading: true });
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.get_permissions',
      params: {},
    });
  },

  addPermission: async (permission) => {
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.add_permission',
      params: permission,
    });
  },

  removePermission: async (id) => {
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.remove_permission',
      params: { id },
    });
  },

  togglePermission: async (id, enabled) => {
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.toggle_permission',
      params: { id, enabled },
    });
  },

  // Blocklist management
  fetchBlocklist: async () => {
    set({ loading: true });
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.get_blocklist',
      params: {},
    });
  },

  addBlocklistEntry: async (entry) => {
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.add_blocklist',
      params: entry,
    });
  },

  removeBlocklistEntry: async (id) => {
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.remove_blocklist',
      params: { id },
    });
  },

  toggleBlocklistEntry: async (id, enabled) => {
    const { send } = useWebSocketStore.getState();
    await send({
      type: 'sudo_command',
      method: 'sudo.toggle_blocklist',
      params: { id, enabled },
    });
  },

  // Update from server
  setPermissions: (permissions) => set({ permissions, loading: false }),
  setBlocklist: (blocklist) => set({ blocklist, loading: false }),
  setLoading: (loading) => set({ loading }),
}));

// ============================================================================
// SELECTOR HOOKS
// ============================================================================

export const usePendingApprovals = () =>
  useSudoStore((state) => state.pendingApprovals.filter((r) => r.status === 'pending'));

export const useSudoPermissions = () =>
  useSudoStore((state) => state.permissions);

export const useSudoBlocklist = () =>
  useSudoStore((state) => state.blocklist);

export const useSudoLoading = () =>
  useSudoStore((state) => state.loading);
