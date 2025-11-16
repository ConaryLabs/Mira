// src/components/PlanDisplay.tsx
// Display generated execution plan with reasoning tokens

import React from 'react';
import { FileText } from 'lucide-react';
import { Plan } from '../stores/useChatStore';

interface PlanDisplayProps {
  plan: Plan;
}

export const PlanDisplay: React.FC<PlanDisplayProps> = ({ plan }) => {
  return (
    <div className="mt-4 border-t border-gray-700 pt-3">
      <div className="flex items-center gap-2 text-sm text-gray-400 mb-2">
        <FileText className="w-4 h-4" />
        <span>Execution Plan Generated</span>
        {plan.reasoning_tokens > 0 && (
          <span className="text-xs text-gray-500">
            ({plan.reasoning_tokens.toLocaleString()} reasoning tokens)
          </span>
        )}
      </div>
      <div className="bg-gray-800/50 rounded-lg p-4 border border-gray-700">
        <div className="text-sm text-slate-100 whitespace-pre-wrap font-mono">
          {plan.plan_text}
        </div>
      </div>
    </div>
  );
};
