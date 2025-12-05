// src/components/SudoApprovalInline.tsx
// Inline approval component for sudo command requests

import React, { useState, useEffect } from 'react';
import { Shield, Check, X, Clock, AlertTriangle, Loader2 } from 'lucide-react';
import { SudoApprovalRequest, useSudoStore } from '../stores/useSudoStore';

interface SudoApprovalInlineProps {
  request: SudoApprovalRequest;
}

export const SudoApprovalInline: React.FC<SudoApprovalInlineProps> = ({ request }) => {
  const [isApproving, setIsApproving] = useState(false);
  const [isDenying, setIsDenying] = useState(false);
  const [showDenyReason, setShowDenyReason] = useState(false);
  const [denyReason, setDenyReason] = useState('');
  const [timeRemaining, setTimeRemaining] = useState<number>(0);

  const { approveRequest, denyRequest } = useSudoStore();

  // Calculate and update time remaining
  useEffect(() => {
    const updateTimer = () => {
      const now = Date.now() / 1000;
      const remaining = Math.max(0, request.expiresAt - now);
      setTimeRemaining(Math.ceil(remaining));
    };

    updateTimer();
    const interval = setInterval(updateTimer, 1000);
    return () => clearInterval(interval);
  }, [request.expiresAt]);

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  };

  const handleApprove = async () => {
    setIsApproving(true);
    try {
      await approveRequest(request.id);
    } catch (error) {
      console.error('Failed to approve:', error);
    } finally {
      setIsApproving(false);
    }
  };

  const handleDeny = async () => {
    if (showDenyReason && !denyReason.trim()) {
      // User clicked deny but hasn't entered a reason - just deny without reason
      setShowDenyReason(false);
    }

    setIsDenying(true);
    try {
      await denyRequest(request.id, denyReason.trim() || undefined);
    } catch (error) {
      console.error('Failed to deny:', error);
    } finally {
      setIsDenying(false);
    }
  };

  const isExpired = timeRemaining === 0;
  const isPending = request.status === 'pending' && !isExpired;
  const isApproved = request.status === 'approved';
  const isDenied = request.status === 'denied' || request.status === 'expired' || isExpired;

  return (
    <div className={`
      my-3 rounded-lg border p-4 transition-all duration-200
      ${isPending ? 'border-yellow-500/50 bg-yellow-500/10' : ''}
      ${isApproved ? 'border-green-500/50 bg-green-500/10' : ''}
      ${isDenied ? 'border-red-500/30 bg-red-500/5' : ''}
    `}>
      {/* Header */}
      <div className="flex items-center gap-2 mb-3">
        <Shield className={`w-5 h-5 ${isPending ? 'text-yellow-500' : isApproved ? 'text-green-500' : 'text-red-500'}`} />
        <span className="font-medium text-sm">
          {isPending && 'Sudo Command Requires Approval'}
          {isApproved && 'Command Approved'}
          {isDenied && (isExpired ? 'Request Expired' : 'Command Denied')}
        </span>
        {isPending && (
          <span className="ml-auto flex items-center gap-1 text-xs text-gray-400">
            <Clock className="w-3 h-3" />
            {formatTime(timeRemaining)}
          </span>
        )}
      </div>

      {/* Command Display */}
      <div className="mb-3">
        <div className="text-xs text-gray-500 mb-1">Command:</div>
        <div className="font-mono text-sm bg-black/30 rounded px-3 py-2 text-green-400 break-all">
          $ {request.command}
        </div>
      </div>

      {/* Reason if provided */}
      {request.reason && (
        <div className="mb-3">
          <div className="text-xs text-gray-500 mb-1">Reason:</div>
          <div className="text-sm text-gray-300">
            {request.reason}
          </div>
        </div>
      )}

      {/* Warning */}
      {isPending && (
        <div className="flex items-start gap-2 mb-3 text-xs text-yellow-400/80">
          <AlertTriangle className="w-4 h-4 flex-shrink-0 mt-0.5" />
          <span>This command will run with elevated privileges. Only approve if you trust this operation.</span>
        </div>
      )}

      {/* Actions */}
      {isPending && (
        <div className="flex items-center gap-2">
          {/* Approve Button */}
          <button
            onClick={handleApprove}
            disabled={isApproving || isDenying}
            className="flex items-center gap-1.5 px-4 py-1.5 rounded bg-green-600 hover:bg-green-500
                       disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium
                       transition-colors"
          >
            {isApproving ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Check className="w-4 h-4" />
            )}
            Approve
          </button>

          {/* Deny Button */}
          {!showDenyReason ? (
            <button
              onClick={() => setShowDenyReason(true)}
              disabled={isApproving || isDenying}
              className="flex items-center gap-1.5 px-4 py-1.5 rounded bg-red-600/80 hover:bg-red-500
                         disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium
                         transition-colors"
            >
              <X className="w-4 h-4" />
              Deny
            </button>
          ) : (
            <div className="flex items-center gap-2 flex-1">
              <input
                type="text"
                placeholder="Reason (optional)"
                value={denyReason}
                onChange={(e) => setDenyReason(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleDeny()}
                className="flex-1 px-2 py-1.5 rounded bg-gray-800 border border-gray-700 text-sm
                           focus:outline-none focus:border-red-500"
                autoFocus
              />
              <button
                onClick={handleDeny}
                disabled={isDenying}
                className="flex items-center gap-1 px-3 py-1.5 rounded bg-red-600 hover:bg-red-500
                           disabled:opacity-50 text-white text-sm font-medium transition-colors"
              >
                {isDenying ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <X className="w-4 h-4" />
                )}
                Deny
              </button>
              <button
                onClick={() => setShowDenyReason(false)}
                className="px-2 py-1.5 rounded text-gray-400 hover:text-white text-sm transition-colors"
              >
                Cancel
              </button>
            </div>
          )}
        </div>
      )}

      {/* Status indicators for resolved states */}
      {isApproved && (
        <div className="flex items-center gap-2 text-green-400 text-sm">
          <Check className="w-4 h-4" />
          Command executed successfully
        </div>
      )}

      {isDenied && !isExpired && (
        <div className="flex items-center gap-2 text-red-400 text-sm">
          <X className="w-4 h-4" />
          Command was denied
        </div>
      )}

      {isExpired && request.status === 'pending' && (
        <div className="flex items-center gap-2 text-gray-400 text-sm">
          <Clock className="w-4 h-4" />
          Request expired - no response received
        </div>
      )}
    </div>
  );
};

export default SudoApprovalInline;
