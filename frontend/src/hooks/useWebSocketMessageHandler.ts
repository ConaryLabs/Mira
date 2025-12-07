// src/hooks/useWebSocketMessageHandler.ts
// REFACTORED: Use shared artifact utilities

import { useEffect } from 'react';
import { useAppState } from '../stores/useAppState';
import { useChatStore } from '../stores/useChatStore';
import { useActivityStore } from '../stores/useActivityStore';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useCodeIntelligenceStore } from '../stores/useCodeIntelligenceStore';
import { useSudoStore } from '../stores/useSudoStore';
import { useUsageStore, PricingTier, WarningLevel, ThinkingStatusType } from '../stores/useUsageStore';
import { createArtifact, extractArtifacts } from '../utils/artifact';

export const useWebSocketMessageHandler = () => {
  const subscribe = useWebSocketStore(state => state.subscribe);
  const send = useWebSocketStore(state => state.send);
  
  const {
    setProjects,
    addModifiedFile,
    clearModifiedFiles,
    setShowFileExplorer,
    addArtifact
  } = useAppState();

  const {
    startStreaming,
    appendStreamContent,
    endStreaming,
    addMessage,
    updateMessagePlan,
    addMessageTask,
    updateTaskStatus,
    setMessageOperationId,
    streamingMessageId,
    addToolExecution
  } = useChatStore();

  const { setCurrentOperation, clearCurrentOperation } = useActivityStore();

  useEffect(() => {
    const unsubscribe = subscribe(
      'global-message-handler',
      (message) => {
        handleMessage(message);
      },
      ['data', 'status', 'error', 'sudo_approval_required', 'sudo_approval_response']
    );

    return unsubscribe;
  }, [subscribe]);

  const handleMessage = (message: any) => {
    if (!message || typeof message !== 'object') {
      console.warn('Received invalid message:', message);
      return;
    }

    switch (message.type) {
      case 'data':
        if (message.data) {
          handleDataMessage(message.data);
        }
        break;
        
      case 'status':
        console.log('Status:', message.message);
        if (message.message && message.message.includes('deleted')) {
          console.log('Project deleted, refreshing list');
          send({
            type: 'project_command',
            method: 'project.list',
            params: {}
          });
        }
        break;
        
      case 'error':
        console.error('Backend error:', message.error || message.message || 'Unknown error');
        break;
        
      case 'heartbeat':
        // Ignore heartbeat messages
        break;

      // SUDO APPROVAL EVENTS
      case 'sudo_approval_required':
        console.log('[WS-Global] Sudo approval required:', message.approval_request_id);
        useSudoStore.getState().addPendingApproval({
          id: message.approval_request_id,
          operationId: message.operation_id,
          sessionId: message.session_id,
          command: message.command,
          reason: message.reason,
          expiresAt: message.expires_at,
          status: 'pending',
          timestamp: Date.now(),
        });
        break;

      case 'sudo_approval_response':
        console.log('[WS-Global] Sudo approval response:', message.approval_request_id, message.status);
        useSudoStore.getState().updateApprovalStatus(
          message.approval_request_id,
          message.status as 'approved' | 'denied' | 'expired'
        );
        // Remove from pending after a short delay to show the status change
        setTimeout(() => {
          useSudoStore.getState().removePendingApproval(message.approval_request_id);
        }, 2000);
        break;

      default:
        console.log('Unhandled message type:', message.type);
        break;
    }
  };

  const handleDataMessage = (data: any) => {
    const dtype = data?.type;
    if (!dtype) return;
    
    console.log('[WS-Global] Handling data type:', dtype);

    switch (dtype) {
      // NEW OPERATION PROTOCOL
      case 'operation.streaming': {
        // Token streaming during response
        if (data.content) {
          appendStreamContent(data.content);
        }
        return;
      }

      case 'operation.artifact_completed': {
        // Artifact completed - add it immediately
        console.log('[WS-Global] Artifact completed:', data.artifact);

        const artifact = createArtifact(data.artifact);
        if (!artifact) {
          console.warn('[WS-Global] Invalid artifact:', data.artifact);
          return;
        }

        console.log('[WS-Global] Adding artifact:', artifact.path);
        addArtifact(artifact);
        return;
      }

      case 'operation.completed': {
        // Operation done - finalize streaming and add message
        console.log('[WS-Global] Operation completed');

        // End streaming (this adds the message)
        endStreaming();

        // Clear thinking status
        useUsageStore.getState().clearThinkingStatus();

        // Artifacts should already be added via operation.artifact_completed
        // But if they're in the final message, add them too
        const artifacts = extractArtifacts(data);
        if (artifacts.length > 0) {
          console.log('[WS-Global] Processing artifacts from completed:', artifacts.length);
          artifacts.forEach(artifact => addArtifact(artifact));
        }

        // Clear current operation from activity panel after a delay
        // (keep it visible for a bit so user can see the completed state)
        setTimeout(() => clearCurrentOperation(), 2000);

        return;
      }

      case 'operation.started': {
        // Operation started - begin streaming
        console.log('[WS-Global] Operation started:', data.operation_id);
        startStreaming();

        // Track operation in activity panel
        // IMPORTANT: Get the NEW streamingMessageId from store after startStreaming()
        const newStreamingId = useChatStore.getState().streamingMessageId;
        if (data.operation_id && newStreamingId) {
          console.log('[WS-Global] Setting current operation:', data.operation_id, newStreamingId);
          setCurrentOperation(data.operation_id, newStreamingId);
        }
        return;
      }

      case 'operation.status_changed': {
        // Status update - log it
        console.log('[WS-Global] Operation status:', data.status);
        return;
      }

      // PLANNING MODE & TASK TRACKING
      case 'operation.plan_generated': {
        // Plan was generated for the operation
        console.log('[WS-Global] Plan generated:', data.operation_id);

        if (streamingMessageId) {
          // Set operation ID on the streaming message
          setMessageOperationId(streamingMessageId, data.operation_id);

          // Update message with plan
          updateMessagePlan(streamingMessageId, {
            plan_text: data.plan_text,
            reasoning_tokens: data.reasoning_tokens,
            timestamp: data.timestamp
          });
        }
        return;
      }

      case 'operation.task_created': {
        // Task was created for tracking
        console.log('[WS-Global] Task created:', data.task_id, data.description);

        if (streamingMessageId) {
          addMessageTask(streamingMessageId, {
            task_id: data.task_id,
            sequence: data.sequence,
            description: data.description,
            active_form: data.active_form,
            status: 'pending',
            timestamp: data.timestamp
          });
        }
        return;
      }

      case 'operation.task_started': {
        // Task execution started
        console.log('[WS-Global] Task started:', data.task_id);

        if (streamingMessageId) {
          updateTaskStatus(streamingMessageId, data.task_id, 'running');
        }
        return;
      }

      case 'operation.task_completed': {
        // Task finished successfully
        console.log('[WS-Global] Task completed:', data.task_id);

        if (streamingMessageId) {
          updateTaskStatus(streamingMessageId, data.task_id, 'completed');
        }
        return;
      }

      case 'operation.task_failed': {
        // Task failed
        console.log('[WS-Global] Task failed:', data.task_id, data.error);

        if (streamingMessageId) {
          updateTaskStatus(streamingMessageId, data.task_id, 'failed', data.error);
        }
        return;
      }

      // TOOL EXECUTION TRACKING
      case 'operation.tool_executed': {
        console.log('[WS-Global] Tool executed:', data.tool_name, data.success ? 'success' : 'failed');

        if (streamingMessageId) {
          addToolExecution(streamingMessageId, {
            toolName: data.tool_name,
            toolType: data.tool_type || 'general',
            summary: data.summary || `Executed ${data.tool_name}`,
            success: data.success ?? true,
            details: data.details,
            timestamp: Date.now(),
          });
        }
        return;
      }

      // AGENT EXECUTION EVENTS
      case 'operation.agent_spawned': {
        console.log('[WS-Global] Agent spawned:', data.agent_name, 'task:', data.task);
        // Display as a tool execution for now (agents show in activity panel)
        if (streamingMessageId) {
          addToolExecution(streamingMessageId, {
            toolName: `Agent: ${data.agent_name}`,
            toolType: 'agent',
            summary: data.task || 'Agent started',
            success: true,
            details: { agent_execution_id: data.agent_execution_id },
            timestamp: Date.now(),
          });
        }
        return;
      }

      case 'operation.agent_progress': {
        console.log('[WS-Global] Agent progress:', data.agent_name, data.current_activity);
        // Could update the agent entry if needed
        return;
      }

      case 'operation.agent_completed': {
        console.log('[WS-Global] Agent completed:', data.agent_name, data.summary);
        if (streamingMessageId) {
          addToolExecution(streamingMessageId, {
            toolName: `Agent: ${data.agent_name}`,
            toolType: 'agent',
            summary: data.summary || 'Agent completed',
            success: true,
            details: { iterations_used: data.iterations_used },
            timestamp: Date.now(),
          });
        }
        return;
      }

      case 'operation.agent_failed': {
        console.log('[WS-Global] Agent failed:', data.agent_name, data.error);
        if (streamingMessageId) {
          addToolExecution(streamingMessageId, {
            toolName: `Agent: ${data.agent_name}`,
            toolType: 'agent',
            summary: data.error || 'Agent failed',
            success: false,
            timestamp: Date.now(),
          });
        }
        return;
      }

      // CODEX (BACKGROUND TASK) EVENTS
      case 'codex.spawned': {
        console.log('[WS-Global] Codex spawned:', data.codex_session_id, data.task_description);
        if (streamingMessageId) {
          addToolExecution(streamingMessageId, {
            toolName: 'Background Task',
            toolType: 'codex',
            summary: data.task_description || 'Background task started',
            success: true,
            details: {
              codex_session_id: data.codex_session_id,
              trigger: data.trigger
            },
            timestamp: Date.now(),
          });
        }
        return;
      }

      case 'codex.progress': {
        console.log('[WS-Global] Codex progress:', data.current_activity, 'tokens:', data.tokens_used);
        // Could update progress indicator if needed
        return;
      }

      case 'codex.completed': {
        console.log('[WS-Global] Codex completed:', data.summary);
        if (streamingMessageId) {
          addToolExecution(streamingMessageId, {
            toolName: 'Background Task',
            toolType: 'codex',
            summary: data.summary || 'Background task completed',
            success: true,
            details: {
              files_changed: data.files_changed,
              duration_seconds: data.duration_seconds,
              tokens_total: data.tokens_total,
              cost_usd: data.cost_usd,
            },
            timestamp: Date.now(),
          });
        }
        return;
      }

      case 'codex.failed': {
        console.log('[WS-Global] Codex failed:', data.error);
        if (streamingMessageId) {
          addToolExecution(streamingMessageId, {
            toolName: 'Background Task',
            toolType: 'codex',
            summary: data.error || 'Background task failed',
            success: false,
            timestamp: Date.now(),
          });
        }
        return;
      }

      // LEGACY: Old artifact_created format
      case 'artifact_created': {
        console.log('[WS-Global] Legacy artifact created:', data.artifact);

        const artifact = createArtifact(data.artifact);
        if (!artifact) {
          console.warn('[WS-Global] artifact_created missing content:', data);
          return;
        }

        addArtifact(artifact);
        return;
      }

      // FILE OPERATIONS
      case 'file_content': {
        const artifact = createArtifact(data, {
          idPrefix: 'file',
          status: 'saved',
          origin: 'user'
        });

        if (!artifact) {
          console.warn('[WS-Global] Invalid file_content:', data);
          return;
        }

        console.log('[WS-Global] File content artifact:', artifact.path);
        addArtifact(artifact);
        setShowFileExplorer(true);
        return;
      }

      // PROJECT MANAGEMENT
      case 'projects': {
        console.log('Projects data received:', data.projects?.length || 0, 'projects');
        if (data.projects) {
          setProjects(data.projects);
        }
        return;
      }

      case 'project_list': {
        console.log('Processing project list:', data.projects?.length || 0, 'projects');
        if (data.projects && Array.isArray(data.projects)) {
          setProjects(data.projects);
          console.log('Projects updated in state');
        }
        return;
      }

      case 'project_created': {
        console.log('Project created:', data.project?.name);
        send({
          type: 'project_command',
          method: 'project.list',
          params: {}
        });
        return;
      }

      case 'local_directory_attached': {
        console.log('Local directory attached:', data.path);
        send({
          type: 'project_command',
          method: 'project.list',
          params: {}
        });
        return;
      }

      // GIT STATUS
      case 'git_status': {
        console.log('Git status update:', data.status);
        if (data.status === 'synced' || data.status === 'modified') {
          if (data.modified_files) {
            clearModifiedFiles();
            data.modified_files.forEach((file: string) => addModifiedFile(file));
          }
        }
        return;
      }

      // CODE INTELLIGENCE - Budget Status
      case 'budget_status': {
        console.log('[WS-Global] Budget status update');
        useCodeIntelligenceStore.getState().setBudget({
          dailyUsagePercent: data.daily_usage_percent,
          monthlyUsagePercent: data.monthly_usage_percent,
          dailySpentUsd: data.daily_spent_usd,
          dailyLimitUsd: data.daily_limit_usd,
          monthlySpentUsd: data.monthly_spent_usd,
          monthlyLimitUsd: data.monthly_limit_usd,
          dailyRemaining: data.daily_remaining,
          monthlyRemaining: data.monthly_remaining,
          isCritical: data.is_critical,
          isLow: data.is_low,
          lastUpdated: data.last_updated || Date.now(),
        });
        return;
      }

      // SUDO PERMISSION MANAGEMENT
      case 'sudo_pending_approvals': {
        console.log('[WS-Global] Sudo pending approvals:', data.approvals?.length || 0);
        // Update the store with the list of pending approvals
        const store = useSudoStore.getState();
        if (data.approvals) {
          data.approvals.forEach((approval: any) => {
            store.addPendingApproval({
              id: approval.id,
              operationId: approval.operation_id,
              sessionId: approval.session_id,
              command: approval.command,
              reason: approval.reason,
              expiresAt: approval.expires_at,
              status: approval.status,
              timestamp: approval.created_at * 1000,
            });
          });
        }
        return;
      }

      case 'sudo_permissions': {
        console.log('[WS-Global] Sudo permissions received:', data.permissions?.length || 0);
        useSudoStore.getState().setPermissions(data.permissions || []);
        return;
      }

      case 'sudo_permission_added':
      case 'sudo_permission_toggled':
      case 'sudo_permission_updated': {
        console.log('[WS-Global] Permission changed, refreshing');
        useSudoStore.getState().fetchPermissions();
        return;
      }

      case 'sudo_blocklist': {
        console.log('[WS-Global] Sudo blocklist received:', data.blocklist?.length || 0);
        useSudoStore.getState().setBlocklist(data.blocklist || []);
        return;
      }

      case 'sudo_blocklist_added':
      case 'sudo_blocklist_toggled': {
        console.log('[WS-Global] Blocklist changed, refreshing');
        useSudoStore.getState().fetchBlocklist();
        return;
      }

      case 'sudo_audit_log': {
        console.log('[WS-Global] Sudo audit log received:', data.entries?.length || 0);
        // For now just log it, could be stored if needed
        return;
      }

      // USAGE & PRICING TIER TRACKING
      case 'operation.usage_info': {
        console.log('[WS-Global] Usage info:', data.pricing_tier, 'cost:', data.cost_usd);
        useUsageStore.getState().updateUsage({
          operationId: data.operation_id,
          tokensInput: data.tokens_input,
          tokensOutput: data.tokens_output,
          pricingTier: data.pricing_tier as PricingTier,
          costUsd: data.cost_usd,
          fromCache: data.from_cache,
          timestamp: data.timestamp || Date.now(),
        });
        return;
      }

      case 'operation.context_warning': {
        console.log('[WS-Global] Context warning:', data.warning_level, data.message);
        useUsageStore.getState().setWarning({
          operationId: data.operation_id,
          warningLevel: data.warning_level as WarningLevel,
          message: data.message,
          tokensInput: data.tokens_input,
          threshold: data.threshold,
          timestamp: data.timestamp || Date.now(),
        });
        return;
      }

      case 'operation.thinking': {
        console.log('[WS-Global] Thinking status:', data.status, data.message);
        useUsageStore.getState().setThinkingStatus({
          operationId: data.operation_id,
          status: data.status as ThinkingStatusType,
          message: data.message,
          tokensIn: data.tokens_in || 0,
          tokensOut: data.tokens_out || 0,
          activeTool: data.active_tool || null,
          timestamp: data.timestamp || Date.now(),
        });
        return;
      }

      default:
        console.log('[WS-Global] Unhandled data type:', dtype);
    }
  };
};
